# Onramp Processor - Quick Reference

## File Locations

| File | Purpose |
|------|---------|
| `src/workers/onramp_processor.rs` | Core processor implementation |
| `src/workers/mod.rs` | Worker module exports |
| `tests/onramp_processor_test.rs` | Test suite |
| `ONRAMP_PROCESSOR_IMPLEMENTATION.md` | Full architecture docs |
| `ONRAMP_PROCESSOR_IMPLEMENTATION_GUIDE.md` | Implementation guide |
| `ONRAMP_PROCESSOR_SUMMARY.md` | High-level overview |

## Key Structs

```rust
pub struct OnrampProcessor {
    db: Arc<PgPool>,
    stellar: Arc<StellarClient>,
    payment_orchestrator: Arc<PaymentOrchestrator>,
    config: OnrampProcessorConfig,
}

pub struct OnrampProcessorConfig {
    pub poll_interval_secs: u64,
    pub pending_timeout_mins: u64,
    pub stellar_max_retries: u32,
    pub stellar_retry_backoff_secs: Vec<u64>,
    pub stellar_confirmation_poll_secs: u64,
    pub stellar_confirmation_timeout_mins: u64,
    pub refund_max_retries: u32,
    pub refund_retry_backoff_secs: Vec<u64>,
}

pub enum FailureReason {
    PaymentTimeout,
    PaymentFailed,
    TrustlineNotFound,
    InsufficientCngnBalance,
    StellarTransientError,
    StellarPermanentError,
    UnknownError,
}
```

## Main Methods

```rust
// Start the worker
pub async fn run(&self, shutdown_rx: watch::Receiver<bool>) -> Result<(), ProcessorError>

// Process payment confirmation (webhook or polling)
pub async fn process_payment_confirmed(
    &self,
    tx_id: &Uuid,
    provider_reference: &str,
    provider: &ProviderName,
    amount_ngn: &BigDecimal,
) -> Result<(), ProcessorError>
```

## Processing Stages

### Stage 1: Payment Confirmation
```rust
async fn check_payment_timeouts() -> Result<(), ProcessorError>
async fn process_pending_transactions() -> Result<(), ProcessorError>
async fn check_payment_with_provider(tx: &Transaction) -> Result<(), ProcessorError>
```

### Stage 2: cNGN Transfer
```rust
async fn execute_cngn_transfer(tx: &Transaction) -> Result<(), ProcessorError>
async fn verify_trustline(wallet_address: &str) -> Result<bool, ProcessorError>
async fn verify_cngn_liquidity(amount: &BigDecimal) -> Result<bool, ProcessorError>
async fn submit_cngn_transfer(tx: &Transaction) -> Result<String, ProcessorError>
async fn attempt_cngn_transfer(tx: &Transaction) -> Result<String, ProcessorError>
```

### Stage 3: Stellar Confirmation
```rust
async fn monitor_stellar_confirmations() -> Result<(), ProcessorError>
async fn check_stellar_confirmation(tx_hash: &str) -> Result<bool, ProcessorError>
```

### Stage 4: Failure & Refunds
```rust
async fn mark_transaction_failed(
    tx_id: &Uuid,
    reason: FailureReason,
    message: &str,
) -> Result<(), ProcessorError>

async fn initiate_refund(
    tx: &Transaction,
    reason: FailureReason,
) -> Result<(), ProcessorError>
```

## Transaction Status Flow

```
pending
  ├─ Payment confirmed ──────────► processing
  ├─ Payment timeout (30 min) ───► failed
  └─ Payment failed ─────────────► failed

processing
  ├─ Stellar confirmed ──────────► completed ✅
  └─ Stellar failed ─────────────► failed
                                   ├─ Refund initiated ──► refunded ✅
                                   └─ Refund failed ─────► pending_manual_review 🚨
```

## Database Queries

### Update Status (Optimistic Locking)
```sql
UPDATE transactions
SET status = 'processing', updated_at = NOW()
WHERE transaction_id = $1 AND status = 'pending'
```

### Fetch Pending Transactions (Polling)
```sql
SELECT * FROM transactions
WHERE status = 'pending'
AND created_at < NOW() - INTERVAL '2 minutes'
FOR UPDATE SKIP LOCKED
LIMIT 50
```

### Fetch Processing Transactions (Stellar Monitoring)
```sql
SELECT * FROM transactions
WHERE status = 'processing'
AND blockchain_tx_hash IS NOT NULL
FOR UPDATE SKIP LOCKED
LIMIT 50
```

## Configuration (Environment Variables)

```bash
# Polling
ONRAMP_POLL_INTERVAL_SECS=30
ONRAMP_PENDING_TIMEOUT_MINS=30

# Stellar Submission
ONRAMP_STELLAR_MAX_RETRIES=3

# System Wallet
STELLAR_SYSTEM_WALLET_ADDRESS=GXXXXXX...
STELLAR_SYSTEM_WALLET_SECRET=SXXXXXX...

# Ops Alerts
SLACK_WEBHOOK_URL=https://hooks.slack.com/...
```

## Error Classification

### Transient Errors (Retry)
- Network timeout
- HTTP 503 (Service Unavailable)
- HTTP 504 (Gateway Timeout)
- Rate limit errors

### Permanent Errors (Fail Fast)
- Invalid sequence number
- Bad signature
- Invalid account
- Insufficient balance (on Stellar)

## Metrics

```
# Counters
onramp_payments_confirmed_total{provider}
onramp_cngn_transfers_submitted_total
onramp_cngn_transfers_confirmed_total
onramp_refunds_initiated_total{provider}
onramp_manual_reviews_total

# Histograms
onramp_payment_confirmation_duration_seconds{provider}
onramp_cngn_transfer_duration_seconds
onramp_stellar_confirmation_duration_seconds
onramp_total_processing_duration_seconds
```

## Logging

All logs include:
- `tx_id` - Transaction ID for correlation
- `correlation_id` - Request correlation ID
- `processor` - "onramp" for filtering
- Structured fields for each stage

Example:
```
INFO: tx_id=550e8400-e29b-41d4-a716-446655440000 processor=onramp Payment timeout detected, marking transaction failed
```

## Performance Targets

| Operation | Target |
|-----------|--------|
| Webhook → processing | < 500ms |
| Payment → cNGN submitted | < 2s |
| Stellar submission → confirmation | < 10s |
| End-to-end | < 30s |
| Polling cycle | Every 30s |
| Refund initiation | < 60s |

## Critical Failure State

### pending_manual_review 🚨

**Condition**: Payment confirmed, cNGN transfer failed, refund initiation failed

**Action**: Immediate ops alert
- User has paid NGN
- User has NOT received cNGN
- Refund attempt failed
- Manual intervention required

**Alert includes**:
- Transaction ID
- User wallet address
- Amount
- Error details

## Integration Checklist

- [ ] Webhook event consumer
- [ ] Provider status query
- [ ] Stellar transaction lookup
- [ ] Trustline verification
- [ ] cNGN liquidity check
- [ ] Stellar transaction submission
- [ ] Refund initiation
- [ ] Ops alert system
- [ ] Prometheus metrics
- [ ] Database schema
- [ ] Environment configuration
- [ ] All tests passing
- [ ] Load testing
- [ ] Staging deployment
- [ ] Production deployment

## Common Issues

### Transaction Stuck in Processing
- Check Stellar transaction hash in logs
- Query Horizon for transaction status
- If confirmed but not marked completed: manual update
- If not confirmed after 5 min: resubmit

### Refund Failed
- Check payment provider API status
- Verify provider credentials
- Check refund amount matches payment
- Retry refund manually if needed

### Payment Timeout
- Check webhook delivery logs
- Verify provider webhook configuration
- Check polling fallback is running
- Increase timeout if needed

### Insufficient cNGN Balance
- Check system wallet balance on Stellar
- Verify cNGN issuer account
- Top up system wallet if needed
- Alert ops team

## Testing

```bash
# Run all tests
cargo test

# Run processor tests only
cargo test onramp_processor

# Run with logging
RUST_LOG=debug cargo test onramp_processor -- --nocapture

# Run integration tests
cargo test --test onramp_processor_test
```

## Deployment

```bash
# Build
cargo build --release

# Run with processor enabled
ONRAMP_PROCESSOR_ENABLED=true cargo run

# Check logs
tail -f logs/aframp.log | grep processor

# Monitor metrics
curl http://localhost:8000/metrics | grep onramp
```

## Useful Commands

```bash
# Check transaction status
SELECT * FROM transactions WHERE transaction_id = 'xxx';

# Check refund status
SELECT * FROM refunds WHERE transaction_id = 'xxx';

# Check webhook events
SELECT * FROM webhook_events WHERE transaction_id = 'xxx';

# Find stuck transactions
SELECT * FROM transactions
WHERE status = 'processing'
AND updated_at < NOW() - INTERVAL '10 minutes';

# Find pending manual reviews
SELECT * FROM transactions
WHERE status = 'pending_manual_review'
ORDER BY created_at DESC;
```

## Documentation Links

- **Full Architecture**: `ONRAMP_PROCESSOR_IMPLEMENTATION.md`
- **Implementation Guide**: `ONRAMP_PROCESSOR_IMPLEMENTATION_GUIDE.md`
- **Summary**: `ONRAMP_PROCESSOR_SUMMARY.md`
- **Code**: `src/workers/onramp_processor.rs`
- **Tests**: `tests/onramp_processor_test.rs`

## Support

For detailed information:
1. Check the relevant documentation file above
2. Review code comments in `src/workers/onramp_processor.rs`
3. Look at test cases in `tests/onramp_processor_test.rs`
4. Check implementation guide for specific TODOs

## Key Principles

✅ **Idempotency**: All operations are safe to retry
✅ **Race Condition Prevention**: Optimistic locking on all updates
✅ **Automatic Refunds**: No user left empty-handed
✅ **Fast Failure**: Permanent errors fail immediately
✅ **Smart Retry**: Transient errors retry with backoff
✅ **Amount Locking**: cNGN amount locked at quote time
✅ **Audit Trail**: Full logging of every state transition
✅ **Ops Alerts**: Immediate notification of critical failures

## Remember

⚠️ **This code moves real money. Every decision matters.**

- Test thoroughly before deployment
- Monitor closely after deployment
- Have runbooks for common issues
- Alert ops team on critical failures
- Never compromise on idempotency
- Always use optimistic locking
- Log everything for debugging
