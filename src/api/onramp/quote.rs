use super::models::*;
use crate::cache::cache::Cache;
use crate::cache::keys::quote::QuoteKey;
use crate::chains::stellar::client::StellarClient;
use crate::chains::stellar::types::is_valid_stellar_address;
use crate::error::{AppError, AppErrorKind, DomainError, ExternalError, ValidationError};
use crate::services::exchange_rate::{
    ConversionDirection, ConversionRequest, ExchangeRateService,
};
use crate::services::fee_calculation::FeeCalculationService;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use chrono::{Duration, Utc};
use serde_json::json;
use sqlx::types::BigDecimal;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

const QUOTE_TTL_SECONDS: i64 = 180; // 3 minutes
const MIN_AMOUNT_NGN: i64 = 1000;
const MAX_AMOUNT_NGN: i64 = 5_000_000;
const CNGN_ASSET_CODE: &str = "cNGN";
const CNGN_ISSUER: &str = "GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"; // TODO: Replace with actual issuer

/// Application state for the quote handler
#[derive(Clone)]
pub struct QuoteHandlerState {
    pub cache: Arc<dyn Cache>,
    pub stellar_client: Arc<StellarClient>,
    pub exchange_rate_service: Arc<ExchangeRateService>,
    pub fee_service: Arc<FeeCalculationService>,
}

/// Handle POST /api/onramp/quote
pub async fn create_quote(
    State(state): State<QuoteHandlerState>,
    Json(request): Json<OnrampQuoteRequest>,
) -> Result<impl IntoResponse, AppError> {
    info!(
        amount_ngn = %request.amount_ngn,
        wallet = %request.wallet_address,
        provider = %request.provider,
        "Processing onramp quote request"
    );

    // 1. Validate request
    validate_quote_request(&request)?;

    // 2. Fetch cached NGN/cNGN rate
    let rate_snapshot_at = Utc::now();
    let conversion_request = ConversionRequest {
        from_currency: "NGN".to_string(),
        to_currency: "cNGN".to_string(),
        amount: request.amount_ngn.clone(),
        direction: ConversionDirection::Buy,
    };

    let conversion_result = state
        .exchange_rate_service
        .calculate_conversion(&conversion_request)
        .await
        .map_err(|e| {
            error!("Failed to fetch exchange rate: {}", e);
            AppError::new(
                AppErrorKind::External(ExternalError::Timeout {
                    service: "rate_service".to_string(),
                    timeout_secs: 30,
                }),
                "Exchange rate service is temporarily unavailable. Please try again.".to_string(),
            )
            .with_status_code(StatusCode::SERVICE_UNAVAILABLE)
            .with_retryable(true)
            .with_details(json!({
                "retry_after": 30
            }))
        })?;

    let rate = BigDecimal::from_str(&conversion_result.base_rate).map_err(|e| {
        error!("Invalid rate format: {}", e);
        AppError::internal_error("Invalid rate format".to_string())
    })?;

    // 3. Calculate gross cNGN
    let gross_cngn = &request.amount_ngn * &rate;

    // 4. Calculate fees
    let fee_breakdown = state
        .fee_service
        .calculate_fees("onramp", request.amount_ngn.clone(), Some(&request.provider), Some("card"))
        .await
        .map_err(|e| {
            error!("Failed to calculate fees: {}", e);
            AppError::internal_error("Failed to calculate fees".to_string())
        })?;

    // Extract fee amounts
    let platform_fee = fee_breakdown.platform.calculated.clone();
    let provider_fee = fee_breakdown
        .provider
        .as_ref()
        .map(|p| p.calculated.clone())
        .unwrap_or_else(|| BigDecimal::from(0));
    let total_fee = &platform_fee + &provider_fee;

    // Calculate platform and provider fee percentages
    let platform_fee_pct = if request.amount_ngn > BigDecimal::from(0) {
        (&platform_fee / &request.amount_ngn) * BigDecimal::from(100)
    } else {
        BigDecimal::from(0)
    };

    let provider_fee_pct = if request.amount_ngn > BigDecimal::from(0) {
        (&provider_fee / &request.amount_ngn) * BigDecimal::from(100)
    } else {
        BigDecimal::from(0)
    };

    // Calculate net amounts
    let amount_ngn_after_fees = &request.amount_ngn - &total_fee;
    let net_cngn = &amount_ngn_after_fees * &rate;

    debug!(
        gross_cngn = %gross_cngn,
        total_fee = %total_fee,
        net_cngn = %net_cngn,
        "Fee calculation complete"
    );

    // 5. Check Stellar liquidity
    let liquidity_available = check_cngn_liquidity(&state.stellar_client, &net_cngn).await?;

    if !liquidity_available {
        warn!(
            requested_cngn = %net_cngn,
            "Insufficient cNGN liquidity"
        );
        return Err(AppError::new(
            AppErrorKind::Domain(DomainError::InvalidAmount {
                amount: net_cngn.to_string(),
                reason: "Insufficient cNGN liquidity available".to_string(),
            }),
            "Insufficient cNGN liquidity for this amount. Try a smaller amount or check back shortly.".to_string(),
        )
        .with_status_code(StatusCode::UNPROCESSABLE_ENTITY)
        .with_details(json!({
            "code": "INSUFFICIENT_LIQUIDITY",
            "requested_cngn": net_cngn.to_string()
        })));
    }

    // 6. Check cNGN trustline
    let trustline_required = check_trustline_status(&state.stellar_client, &request.wallet_address).await?;

    // 7. Generate quote_id and build quote
    let quote_id = Uuid::new_v4();
    let created_at = Utc::now();
    let expires_at = created_at + Duration::seconds(QUOTE_TTL_SECONDS);
    let expires_in_seconds = QUOTE_TTL_SECONDS;

    let fees = QuoteFeeBreakdown {
        platform_fee_ngn: platform_fee.to_string(),
        provider_fee_ngn: provider_fee.to_string(),
        total_fee_ngn: total_fee.to_string(),
        platform_fee_pct: format!("{:.2}", platform_fee_pct),
        provider_fee_pct: format!("{:.2}", provider_fee_pct),
    };

    let stored_quote = StoredQuote {
        quote_id,
        status: QuoteStatus::Pending,
        wallet_address: request.wallet_address.clone(),
        amount_ngn: request.amount_ngn.to_string(),
        amount_cngn: net_cngn.to_string(),
        rate_snapshot: rate.to_string(),
        rate_snapshot_at,
        fees: fees.clone(),
        provider: request.provider.clone(),
        chain: request.chain.clone(),
        trustline_required,
        created_at,
        expires_at,
    };

    // 8. Persist quote to Redis
    let quote_key = QuoteKey::new(quote_id.to_string());
    let quote_json = serde_json::to_string(&stored_quote).map_err(|e| {
        error!("Failed to serialize quote: {}", e);
        AppError::internal_error("Failed to create quote".to_string())
    })?;

    state
        .cache
        .set(&quote_key.to_string(), &quote_json, Some(QUOTE_TTL_SECONDS as u64))
        .await
        .map_err(|e| {
            error!("Failed to store quote in Redis: {}", e);
            AppError::internal_error("Failed to store quote".to_string())
        })?;

    info!(
        quote_id = %quote_id,
        expires_at = %expires_at,
        "Quote created successfully"
    );

    // 9. Build and return response
    let response = OnrampQuoteResponse {
        quote_id,
        expires_at,
        expires_in_seconds,
        input: QuoteInput {
            amount_ngn: request.amount_ngn.to_string(),
            provider: request.provider,
            chain: request.chain.clone(),
            wallet_address: request.wallet_address,
        },
        fees,
        output: QuoteOutput {
            amount_ngn_after_fees: amount_ngn_after_fees.to_string(),
            rate: rate.to_string(),
            rate_source: "fixed_peg".to_string(),
            rate_snapshot_at,
            amount_cngn: net_cngn.to_string(),
            chain: request.chain,
        },
        trustline_required,
        liquidity_available: true,
    };

    Ok((StatusCode::OK, Json(response)))
}

/// Validate the quote request
fn validate_quote_request(request: &OnrampQuoteRequest) -> Result<(), AppError> {
    // Validate amount range
    let amount_i64 = request.amount_ngn.to_string().parse::<f64>().map_err(|_| {
        AppError::new(
            AppErrorKind::Validation(ValidationError::InvalidAmount {
                amount: request.amount_ngn.to_string(),
                reason: "Invalid amount format".to_string(),
            }),
            "Amount must be a valid number".to_string(),
        )
        .with_status_code(StatusCode::BAD_REQUEST)
    })?;

    if amount_i64 < MIN_AMOUNT_NGN as f64 {
        return Err(AppError::new(
            AppErrorKind::Validation(ValidationError::OutOfRange {
                field: "amount_ngn".to_string(),
                min: Some(MIN_AMOUNT_NGN.to_string()),
                max: Some(MAX_AMOUNT_NGN.to_string()),
            }),
            format!("Minimum onramp amount is ₦{:,}.", MIN_AMOUNT_NGN),
        )
        .with_status_code(StatusCode::BAD_REQUEST)
        .with_details(json!({
            "code": "AMOUNT_TOO_LOW",
            "minimum_amount_ngn": MIN_AMOUNT_NGN
        })));
    }

    if amount_i64 > MAX_AMOUNT_NGN as f64 {
        return Err(AppError::new(
            AppErrorKind::Validation(ValidationError::OutOfRange {
                field: "amount_ngn".to_string(),
                min: Some(MIN_AMOUNT_NGN.to_string()),
                max: Some(MAX_AMOUNT_NGN.to_string()),
            }),
            format!("Maximum onramp amount is ₦{:,} per transaction.", MAX_AMOUNT_NGN),
        )
        .with_status_code(StatusCode::BAD_REQUEST)
        .with_details(json!({
            "code": "AMOUNT_TOO_HIGH",
            "maximum_amount_ngn": MAX_AMOUNT_NGN
        })));
    }

    // Validate wallet address
    if !is_valid_stellar_address(&request.wallet_address) {
        return Err(AppError::new(
            AppErrorKind::Validation(ValidationError::InvalidWalletAddress {
                address: request.wallet_address.clone(),
                reason: "Not a valid Stellar public key".to_string(),
            }),
            "Wallet address is not a valid Stellar public key.".to_string(),
        )
        .with_status_code(StatusCode::BAD_REQUEST)
        .with_details(json!({
            "code": "INVALID_WALLET",
            "provided": request.wallet_address
        })));
    }

    // Validate provider
    let valid_providers = ["flutterwave", "paystack", "mpesa"];
    if !valid_providers.contains(&request.provider.to_lowercase().as_str()) {
        return Err(AppError::new(
            AppErrorKind::Validation(ValidationError::InvalidCurrency {
                currency: request.provider.clone(),
                reason: "Unsupported provider".to_string(),
            }),
            format!("Provider '{}' is not supported.", request.provider),
        )
        .with_status_code(StatusCode::BAD_REQUEST)
        .with_details(json!({
            "code": "INVALID_PROVIDER",
            "supported_providers": valid_providers
        })));
    }

    Ok(())
}

/// Check cNGN liquidity on Stellar
async fn check_cngn_liquidity(
    stellar_client: &Arc<StellarClient>,
    amount: &BigDecimal,
) -> Result<bool, AppError> {
    // TODO: Implement actual liquidity check via Stellar SDK
    // For now, we'll assume liquidity is available
    // In production, this should query the Stellar orderbook or liquidity pool
    
    debug!(
        amount = %amount,
        "Checking cNGN liquidity (placeholder implementation)"
    );

    // Placeholder: Always return true for now
    // Real implementation would check:
    // 1. Query Stellar Horizon for cNGN asset liquidity
    // 2. Check orderbook depth
    // 3. Verify issuer account has sufficient balance
    Ok(true)
}

/// Check if wallet has cNGN trustline
async fn check_trustline_status(
    stellar_client: &Arc<StellarClient>,
    wallet_address: &str,
) -> Result<bool, AppError> {
    debug!(
        wallet = %wallet_address,
        "Checking cNGN trustline status"
    );

    match stellar_client.get_account(wallet_address).await {
        Ok(account_info) => {
            // Check if account has cNGN trustline
            let has_trustline = account_info.balances.iter().any(|balance| {
                balance.asset_code.as_deref() == Some(CNGN_ASSET_CODE)
                    && balance.asset_issuer.as_deref() == Some(CNGN_ISSUER)
            });

            Ok(!has_trustline) // Return true if trustline is required (not found)
        }
        Err(e) => {
            warn!(
                wallet = %wallet_address,
                error = %e,
                "Failed to fetch account info, assuming trustline required"
            );
            // If account doesn't exist or we can't fetch it, assume trustline is required
            Ok(true)
        }
    }
}
