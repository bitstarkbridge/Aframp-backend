# Onramp Processor Implementation Guide

## Overview

This guide provides step-by-step instructions for completing the Onramp Transaction Processor implementation. The core structure is in place; this document covers the remaining integration points and TODO items.

## Completed ✅

- [x] Core worker struct and configuration
- [x] Main processing loop with graceful shutdown
- [x] Payment timeout detection (Stage 1)
- [x] Polling fallback loop (Stage 1)
- [x] Stellar confirmation monitoring (Stage 3)
- [x] Transaction status transitions with optimistic locking
- [x] Failure handling framework
- [x] Refund initiation framework
- [x] Structured logging with correlation IDs
- [x] Metrics emission placeholders
- [x] Comprehensive tests
- [x] Documentation

## Remaining TODOs

### 1. Webhook Event Consumer Integration

**Location**: `src/workers/onramp_processor.rs` - `process_payment_confirmed()` method

**What to do**:
- Create a webhook event channel/queue consumer
- Listen for payment confirmation events from the webhook processing system (#21)
- Extract provider_reference, provider, and amount_ngn from webhook payload
- Call `processor.process_payment_confirmed()` with extracted data

**Implementation approach**:
```rust
// In main.rs or a separate webhook consumer module
let (webhook_tx, mut webhook_rx) = tokio::sync::mpsc::channel(1000);

// Spawn webhook consumer task
tokio::spawn(async move {
    while let Some(event) = webhook_rx.recv().await {
        if let Err(e) = processor.process_payment_confirmed(
            &event.tx_id,
            &event.provider_reference,
            &event.provider,
            &event.amount_ngn,
        ).await {
            error!(error = %e, "Failed to process payment confirmation");
        }
    }
});
```

**Integration points**:
- Webhook repository to fetch transaction by provider_reference
- Payment orchestrator to verify payment status

---

### 2. Provider Status Query (Polling Fallback)

**Location**: `src/workers/onramp_processor.rs` - `check_payment_with_provider()` method

**What to do**:
- Implement provider status query using PaymentOrchestrator
- Call provider's status endpoint with provider_reference
- Parse response to determine if payment is confirmed
- If confirmed, call `process_payment_confirmed()`

**Implementation approach**:
```rust
async fn check_payment_with_provider(&self, tx: &Transaction) -> Result<(), ProcessorError> {
    let provider_ref = tx.payment_reference.as_ref().ok_or(...)?;
    let provider_name = tx.payment_provider.as_ref().ok_or(...)?;
    
    let provider = ProviderName::from_str(provider_name)?;
    
    // Use payment orchestrator to check status
    let status_request = StatusRequest {
        transaction_reference: Some(tx.transaction_id.to_string()),
        provider_reference: Some(provider_ref.clone()),
    };
    
    match self.payment_orchestrator.verify_payment(provider, status_request).await {
        Ok(response) => {
            if response.status == PaymentState::Success {
                // Payment confirmed
                self.process_payment_confirmed(
                    &tx.transaction_id,
                    provider_ref,
                    &provider,
                    &tx.from_amount,
                ).await?;
            }
        }
        Err(e) => {
            warn!(error = %e, "Provider status check failed");
        }
    }
    
    Ok(())
}
```

**Integration points**:
- PaymentOrchestrator.verify_payment()
- Payment provider implementations (Flutterwave, Paystack, M-Pesa)

---

### 3. Stellar Transaction Lookup

**Location**: `src/workers/onramp_processor.rs` - `check_stellar_confirmation()` method

**What to do**:
- Query Stellar Horizon API for transaction status
- Check if transaction is confirmed (included in ledger)
- Return true if confirmed, false if still pending

**Implementation approach**:
```rust
async fn check_stellar_confirmation(&self, tx_hash: &str) -> Result<bool, ProcessorError> {
    debug!(stellar_hash = %tx_hash, "Querying Stellar for transaction confirmation");
    
    // Use StellarClient to fetch transaction
    match self.stellar.get_transaction(tx_hash).await {
        Ok(tx_record) => {
            if tx_record.successful {
                debug!(
                    stellar_hash = %tx_hash,
                    ledger = tx_record.ledger,
                    "Transaction confirmed on Stellar"
                );
                Ok(true)
            } else {
                debug!(stellar_hash = %tx_hash, "Transaction failed on Stellar");
                Ok(false)
            }
        }
        Err(e) => {
            // Transaction not found yet (still pending)
            debug!(error = %e, "Transaction not yet confirmed");
            Ok(false)
        }
    }
}
```

**Integration points**:
- StellarClient.get_transaction()
- Horizon API transaction endpoint

---

### 4. Trustline Verification

**Location**: `src/workers/onramp_processor.rs` - `verify_trustline()` method

**What to do**:
- Check if wallet has active cNGN trustline
- Use CngnTrustlineService to verify
- Return true if trustline exists and is authorized, false otherwise

**Implementation approach**:
```rust
async fn verify_trustline(&self, wallet_address: &str) -> Result<bool, ProcessorError> {
    debug!(wallet = %wallet_address, "Verifying cNGN trustline");
    
    // Use trustline service to check
    let trustline_service = CngnTrustlineService::new(self.stellar.clone());
    
    match trustline_service.check_trustline(wallet_address).await {
        Ok(status) => {
            if status.exists && status.is_authorized {
                debug!(wallet = %wallet_address, "Trustline verified");
                Ok(true)
            } else {
                warn!(wallet = %wallet_address, "Trustline missing or unauthorized");
                Ok(false)
            }
        }
        Err(e) => {
            error!(wallet = %wallet_address, error = %e, "Trustline check failed");
            Err(ProcessorError::Stellar(e.to_string()))
        }
    }
}
```

**Integration points**:
- CngnTrustlineService.check_trustline()
- StellarClient.get_account()

---

### 5. cNGN Liquidity Check

**Location**: `src/workers/onramp_processor.rs` - `verify_cngn_liquidity()` method

**What to do**:
- Query Stellar for system wallet cNGN balance
- Compare against required amount
- Return true if sufficient, false otherwise

**Implementation approach**:
```rust
async fn verify_cngn_liquidity(&self, amount: &BigDecimal) -> Result<bool, ProcessorError> {
    debug!(amount = %amount, "Verifying cNGN liquidity");
    
    let system_wallet = std::env::var("STELLAR_SYSTEM_WALLET_ADDRESS")
        .map_err(|_| ProcessorError::Internal("System wallet not configured".to_string()))?;
    
    // Get account and extract cNGN balance
    match self.stellar.get_account(&system_wallet).await {
        Ok(account) => {
            // Find cNGN balance
            let cngn_balance = account.balances
                .iter()
                .find(|b| b.asset_code == "cNGN")
                .map(|b| BigDecimal::from_str(&b.balance).unwrap_or_default())
                .unwrap_or_default();
            
            if cngn_balance >= *amount {
                debug!(
                    available = %cngn_balance,
                    required = %amount,
                    "Sufficient cNGN liquidity"
                );
                Ok(true)
            } else {
                warn!(
                    available = %cngn_balance,
                    required = %amount,
                    "Insufficient cNGN liquidity"
                );
                Ok(false)
            }
        }
        Err(e) => {
            error!(error = %e, "Failed to check cNGN liquidity");
            Err(ProcessorError::Stellar(e.to_string()))
        }
    }
}
```

**Integration points**:
- StellarClient.get_account()
- Environment configuration for system wallet address

---

### 6. Stellar Transaction Submission

**Location**: `src/workers/onramp_processor.rs` - `attempt_cngn_transfer()` method

**What to do**:
- Build cNGN payment transaction using CngnPaymentBuilder
- Sign with system wallet private key
- Submit to Stellar
- Return transaction hash on success
- Distinguish transient vs permanent errors

**Implementation approach**:
```rust
async fn attempt_cngn_transfer(&self, tx: &Transaction) -> Result<String, ProcessorError> {
    debug!(
        tx_id = %tx.transaction_id,
        wallet = %tx.wallet_address,
        amount = %tx.cngn_amount,
        "Attempting cNGN transfer"
    );
    
    let builder = CngnPaymentBuilder::new(self.stellar.clone());
    
    // Build transaction
    let draft = builder.build_payment(
        &tx.wallet_address,
        &tx.cngn_amount.to_string(),
        Some(tx.transaction_id.to_string()), // memo
    ).await.map_err(|e| ProcessorError::Stellar(e.to_string()))?;
    
    // Sign transaction
    let secret_seed = std::env::var("STELLAR_SYSTEM_WALLET_SECRET")
        .map_err(|_| ProcessorError::Internal("System wallet secret not configured".to_string()))?;
    
    let signed = builder.sign_payment(&draft, &secret_seed)
        .map_err(|e| ProcessorError::Stellar(e.to_string()))?;
    
    // Submit to Stellar
    match builder.submit_payment(&signed).await {
        Ok(response) => {
            let tx_hash = response.hash.clone();
            info!(
                tx_id = %tx.transaction_id,
                stellar_hash = %tx_hash,
                "cNGN transfer submitted successfully"
            );
            Ok(tx_hash)
        }
        Err(e) => {
            // Classify error as transient or permanent
            if is_transient_stellar_error(&e) {
                Err(ProcessorError::StellarTransientError(e.to_string()))
            } else {
                Err(ProcessorError::StellarPermanentError(e.to_string()))
            }
        }
    }
}

fn is_transient_stellar_error(error: &str) -> bool {
    // Network timeouts, rate limits, server errors
    error.contains("timeout")
        || error.contains("503")
        || error.contains("504")
        || error.contains("rate limit")
}
```

**Integration points**:
- CngnPaymentBuilder.build_payment()
- CngnPaymentBuilder.sign_payment()
- CngnPaymentBuilder.submit_payment()
- StellarClient transaction submission

---

### 7. Refund Initiation

**Location**: `src/workers/onramp_processor.rs` - `initiate_refund()` method

**What to do**:
- Call PaymentOrchestrator to initiate refund
- Handle provider-specific refund methods (Flutterwave, Paystack, M-Pesa)
- Track refund status in database
- Alert ops on failure

**Implementation approach**:
```rust
async fn initiate_refund(
    &self,
    tx: &Transaction,
    reason: FailureReason,
) -> Result<(), ProcessorError> {
    info!(
        tx_id = %tx.transaction_id,
        provider = ?tx.payment_provider,
        reason = %reason.as_str(),
        "Initiating automatic refund"
    );
    
    let provider_name = tx.payment_provider.as_ref()
        .ok_or_else(|| ProcessorError::Internal("No payment provider".to_string()))?;
    
    let provider = ProviderName::from_str(provider_name)?;
    let provider_ref = tx.payment_reference.as_ref()
        .ok_or_else(|| ProcessorError::Internal("No payment reference".to_string()))?;
    
    // Create refund record in database
    let refund_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO refunds (refund_id, transaction_id, provider, provider_reference, amount, status)
        VALUES ($1, $2, $3, $4, $5, 'initiated')
        "#,
    )
    .bind(refund_id)
    .bind(&tx.transaction_id)
    .bind(provider_name)
    .bind(provider_ref)
    .bind(&tx.from_amount)
    .execute((*self.db).as_ref())
    .await?;
    
    // Call payment orchestrator to initiate refund
    match self.payment_orchestrator.initiate_refund(
        provider,
        provider_ref,
        &tx.from_amount,
    ).await {
        Ok(_) => {
            info!(
                tx_id = %tx.transaction_id,
                refund_id = %refund_id,
                "Refund initiated successfully"
            );
            
            // Update transaction status to refunded
            sqlx::query(
                r#"
                UPDATE transactions
                SET status = 'refunded', updated_at = NOW()
                WHERE transaction_id = $1
                "#,
            )
            .bind(&tx.transaction_id)
            .execute((*self.db).as_ref())
            .await?;
            
            metrics::counter!("onramp_refunds_completed_total", "provider" => provider.as_str())
                .increment(1);
            
            Ok(())
        }
        Err(e) => {
            error!(
                tx_id = %tx.transaction_id,
                refund_id = %refund_id,
                error = %e,
                "Refund initiation failed"
            );
            
            // Mark as pending_manual_review
            sqlx::query(
                r#"
                UPDATE transactions
                SET status = 'pending_manual_review', updated_at = NOW()
                WHERE transaction_id = $1
                "#,
            )
            .bind(&tx.transaction_id)
            .execute((*self.db).as_ref())
            .await?;
            
            // Alert ops team
            self.alert_ops_team(
                &tx.transaction_id,
                "Refund failed - manual intervention required",
                &e.to_string(),
            ).await;
            
            metrics::counter!("onramp_manual_reviews_total").increment(1);
            
            Err(ProcessorError::RefundFailed(e.to_string()))
        }
    }
}

async fn alert_ops_team(&self, tx_id: &Uuid, title: &str, details: &str) {
    // TODO: Implement Slack/PagerDuty alert
    error!(
        tx_id = %tx_id,
        title = %title,
        details = %details,
        "🚨 OPS ALERT: Manual intervention required"
    );
}
```

**Integration points**:
- PaymentOrchestrator.initiate_refund()
- Refund database table
- Slack/PagerDuty API for ops alerts

---

### 8. Prometheus Metrics

**Location**: `src/workers/onramp_processor.rs` - Replace metrics placeholder module

**What to do**:
- Replace the mock metrics module with actual prometheus integration
- Use `prometheus` crate for counters and histograms
- Emit metrics at every key stage

**Implementation approach**:
```rust
// In Cargo.toml
prometheus = "0.13"

// In src/workers/onramp_processor.rs
use prometheus::{Counter, CounterVec, Histogram, HistogramVec, Registry};

pub struct ProcessorMetrics {
    payments_confirmed: CounterVec,
    payments_failed: CounterVec,
    cngn_transfers_submitted: Counter,
    cngn_transfers_confirmed: Counter,
    cngn_transfers_failed: CounterVec,
    refunds_initiated: CounterVec,
    refunds_completed: CounterVec,
    refunds_failed: CounterVec,
    manual_reviews: Counter,
    
    payment_confirmation_duration: HistogramVec,
    cngn_transfer_duration: Histogram,
    stellar_confirmation_duration: Histogram,
    total_processing_duration: Histogram,
}

impl ProcessorMetrics {
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let payments_confirmed = CounterVec::new(
            prometheus::Opts::new("onramp_payments_confirmed_total", "Total confirmed payments"),
            &["provider"],
        )?;
        registry.register(Box::new(payments_confirmed.clone()))?;
        
        // ... register other metrics
        
        Ok(Self {
            payments_confirmed,
            // ...
        })
    }
}
```

**Integration points**:
- Prometheus registry
- Metrics endpoint in main.rs

---

### 9. Database Schema Updates

**What to do**:
- Add `refunds` table if not exists
- Add `failure_reason` column to transactions if not exists
- Add indexes for performance

**Migration SQL**:
```sql
-- Create refunds table
CREATE TABLE IF NOT EXISTS refunds (
    refund_id UUID PRIMARY KEY,
    transaction_id UUID NOT NULL REFERENCES transactions(transaction_id),
    provider VARCHAR(50) NOT NULL,
    provider_reference VARCHAR(255) NOT NULL,
    amount DECIMAL(20, 8) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'initiated',
    retry_count INT DEFAULT 0,
    last_retry_at TIMESTAMP,
    error_message TEXT,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

-- Add columns to transactions if not exists
ALTER TABLE transactions
ADD COLUMN IF NOT EXISTS failure_reason VARCHAR(100),
ADD COLUMN IF NOT EXISTS provider_confirmed_at TIMESTAMP,
ADD COLUMN IF NOT EXISTS failed_at TIMESTAMP,
ADD COLUMN IF NOT EXISTS refunded_at TIMESTAMP;

-- Add indexes
CREATE INDEX IF NOT EXISTS idx_transactions_status_created ON transactions(status, created_at);
CREATE INDEX IF NOT EXISTS idx_transactions_blockchain_hash ON transactions(blockchain_tx_hash);
CREATE INDEX IF NOT EXISTS idx_refunds_transaction_id ON refunds(transaction_id);
CREATE INDEX IF NOT EXISTS idx_refunds_status ON refunds(status);
```

---

### 10. Environment Configuration

**What to do**:
- Add environment variables to `.env.example`
- Document all configuration options

**Add to `.env.example`**:
```bash
# Onramp Processor
ONRAMP_PROCESSOR_ENABLED=true
ONRAMP_POLL_INTERVAL_SECS=30
ONRAMP_PENDING_TIMEOUT_MINS=30
ONRAMP_STELLAR_MAX_RETRIES=3

# Stellar System Wallet
STELLAR_SYSTEM_WALLET_ADDRESS=GXXXXXX...
STELLAR_SYSTEM_WALLET_SECRET=SXXXXXX...

# Ops Alerts
SLACK_WEBHOOK_URL=https://hooks.slack.com/...
PAGERDUTY_API_KEY=...
```

---

## Integration Checklist

- [ ] Webhook event consumer integrated
- [ ] Provider status query implemented
- [ ] Stellar transaction lookup implemented
- [ ] Trustline verification implemented
- [ ] cNGN liquidity check implemented
- [ ] Stellar transaction submission implemented
- [ ] Refund initiation implemented
- [ ] Ops alert system implemented
- [ ] Prometheus metrics integrated
- [ ] Database schema updated
- [ ] Environment configuration added
- [ ] All tests passing
- [ ] Integration tests passing
- [ ] Load testing completed
- [ ] Production deployment ready

## Testing Strategy

1. **Unit Tests**: Test individual methods in isolation
2. **Integration Tests**: Test full workflows (webhook → completion, payment → refund)
3. **Load Tests**: Simulate high transaction volume
4. **Chaos Tests**: Simulate provider failures, network issues, Stellar errors
5. **Manual Testing**: End-to-end testing with real payment providers (testnet)

## Deployment Steps

1. Deploy database migrations
2. Deploy updated code with processor
3. Enable processor via `ONRAMP_PROCESSOR_ENABLED=true`
4. Monitor metrics and logs
5. Gradually increase transaction volume
6. Set up ops alerts
7. Document runbooks for common issues

## Monitoring & Alerting

Set up alerts for:
- Payment confirmation rate drops
- Failure rate exceeds 5%
- Manual review queue grows
- Processing duration exceeds 60s p99
- Refund success rate < 99%

## Support & Troubleshooting

See `ONRAMP_PROCESSOR_IMPLEMENTATION.md` for:
- Architecture overview
- Processing pipeline details
- Failure scenarios
- Performance targets
- Critical failure states
