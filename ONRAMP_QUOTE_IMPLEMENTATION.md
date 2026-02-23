# Onramp Quote Endpoint Implementation

## Files Created

### 1. `src/api/onramp/mod.rs`
Module declaration for onramp API components.

### 2. `src/api/onramp/models.rs`
Data models for the quote endpoint:
- `OnrampQuoteRequest` - Request payload
- `OnrampQuoteResponse` - Success response
- `QuoteInput`, `QuoteFeeBreakdown`, `QuoteOutput` - Response components
- `StoredQuote` - Redis storage format
- `QuoteStatus` - Enum (Pending, Consumed)
- `Chain` - Enum (Stellar)

### 3. `src/api/onramp/quote.rs`
Main handler implementation with:
- `create_quote()` - POST /api/onramp/quote handler
- `QuoteHandlerState` - Handler dependencies
- Request validation (amount range, wallet address, provider)
- Rate fetching from ExchangeRateService
- Fee calculation via FeeCalculationService
- Liquidity checking (placeholder)
- Trustline status checking
- Quote storage in Redis with 180s TTL
- Comprehensive error handling

### 4. `src/cache/keys.rs` (Updated)
Added `quote` module with `QuoteKey` for type-safe Redis keys.

### 5. `src/api/mod.rs` (Updated)
Added `pub mod onramp;` to expose the onramp module.

## Implementation Details

### Constants
- `QUOTE_TTL_SECONDS`: 180 (3 minutes)
- `MIN_AMOUNT_NGN`: 1,000
- `MAX_AMOUNT_NGN`: 5,000,000

### Validation Rules
- Amount must be between ₦1,000 and ₦5,000,000
- Wallet address must be valid Stellar public key (G...)
- Provider must be one of: flutterwave, paystack, mpesa

### Error Responses
- `AMOUNT_TOO_LOW` (400) - Below minimum
- `AMOUNT_TOO_HIGH` (400) - Above maximum
- `INVALID_WALLET` (400) - Invalid Stellar address
- `INVALID_PROVIDER` (400) - Unsupported provider
- `INSUFFICIENT_LIQUIDITY` (422) - Not enough cNGN available
- `RATE_SERVICE_UNAVAILABLE` (503) - Exchange rate service down

### Quote Storage
Quotes are stored in Redis with key format: `v1:quote:{quote_id}`
- TTL: 180 seconds
- Status: pending (can be marked as consumed later)
- Contains: amount, rate snapshot, fees, wallet, provider, chain

## Integration Required

To integrate this endpoint into your application, add to `src/main.rs`:

```rust
// After wallet_routes setup, add:

// Setup onramp routes with quote service
let onramp_routes = if let (Some(pool), Some(cache), Some(client)) = (
    db_pool.clone(),
    redis_cache.clone(),
    stellar_client.clone(),
) {
    use crate::services::exchange_rate::{ExchangeRateService, ExchangeRateServiceConfig};
    use crate::services::fee_calculation::FeeCalculationService;
    use crate::database::exchange_rate_repository::ExchangeRateRepository;
    use crate::services::rate_providers::FixedRateProvider;

    // Initialize exchange rate service
    let rate_repo = ExchangeRateRepository::new(pool.clone());
    let rate_provider = std::sync::Arc::new(FixedRateProvider::new());
    let exchange_rate_service = std::sync::Arc::new(
        ExchangeRateService::new(rate_repo, ExchangeRateServiceConfig::default())
            .with_cache(cache.clone())
            .add_provider(rate_provider)
    );

    // Initialize fee calculation service
    let fee_service = std::sync::Arc::new(FeeCalculationService::new(pool.clone()));

    let quote_state = api::onramp::quote::QuoteHandlerState {
        cache: std::sync::Arc::new(cache) as std::sync::Arc<dyn crate::cache::cache::Cache>,
        stellar_client: std::sync::Arc::new(client),
        exchange_rate_service,
        fee_service,
    };

    Router::new()
        .route("/api/onramp/quote", post(api::onramp::quote::create_quote))
        .with_state(quote_state)
} else {
    info!("⏭️  Skipping onramp routes (missing dependencies)");
    Router::new()
};

// Then in the main router, add:
.merge(onramp_routes)
```

## TODO Items

1. **cNGN Issuer Address**: Update `CNGN_ISSUER` constant in `quote.rs` with actual issuer address
2. **Liquidity Check**: Implement actual Stellar liquidity checking in `check_cngn_liquidity()`
3. **Trustline Check**: Verify the trustline checking logic matches your cNGN asset configuration
4. **Testing**: Add unit and integration tests
5. **Rate Limiting**: Consider adding rate limiting per IP (20 req/min suggested)
6. **Monitoring**: Add metrics for quote-to-initiate conversion rate

## Dependencies Used

- `axum` - Web framework
- `serde` / `serde_json` - Serialization
- `sqlx` - Database (BigDecimal type)
- `uuid` - Quote ID generation
- `chrono` - Timestamps
- `tracing` - Logging

## Next Steps

After integration:
1. Test with curl or Postman
2. Verify Redis storage and TTL
3. Implement `/api/onramp/initiate` endpoint to consume quotes
4. Add monitoring and alerting
