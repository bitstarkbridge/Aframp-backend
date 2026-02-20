# Onramp Transaction Processor Implementation

## Overview

The Onramp Transaction Processor is the engine room of the entire onramp flow. It's a background worker that runs continuously, picking up pending transactions after `/onramp/initiate` hands them off, watching for payment confirmations from providers, executing cNGN transfers on Stellar, and handling every failure scenario including refunds.

**Status**: ✅ Core implementation complete
**Location**: `src/workers/onramp_processor.rs`
**Tests**: `tests/onramp_processor_test.rs`

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                  Onramp Transaction Processor                │
│                                                              │
│  ┌─────────────────┐      ┌──────────────────────────────┐  │
│  │  Webhook Handler │      │     Polling Fallback Loop    │  │
│  │                  │      │                              │  │
│  │ POST /webhooks/  │      │ Every 30s: scan pending txs  │  │
│  │ flutterwave      │      │ older than 2min with no      │  │
│  │ paystack         │      │ webhook — query provider     │  │
│  │ mpesa            │      │ directly                     │  │
│  └────────┬─────────┘      └──────────────┬───────────────┘  │
│           │                               │                  │
│           └───────────────┬───────────────┘                  │
│                           ▼                                  │
│              ┌────────────────────────┐                      │
│              │  Payment Confirmation  │                      │
│              │      Processor         │                      │
│              └────────────┬───────────┘                      │
│                           │ Confirmed                        │
│                           ▼                                  │
│              ┌────────────────────────┐                      │
│              │   cNGN Transfer        │                      │
│              │   Executor (Stellar)   │                      │
│              └────────────┬───────────┘                      │
│                           │                                  │
│              ┌────────────┴───────────┐                      │
│              │ Success    │  Failure  │                      │
│              ▼            ▼           │                      │
│        ┌──────────┐  ┌─────────┐     │                      │
│        │ Mark     │  │ Retry / │     │                      │
│        │ completed│  │ Refund  │     │                      │
│        └──────────┘  └─────────┘     │                      │
│                                       │                      │
└─────────────────────────────────────────────────────────────┘
```

## Processing Pipeline

### Stage 1: Payment Confirmation

**Trigger sources** (two paths):

**Path A — Webhook (preferred)**:
- Provider sends webhook to `/webhooks/{provider}`
- Webhook processor verifies signature, parses event, publishes to internal queue
- Processor picks up the event and looks up transaction by `provider_reference`

**Path B — Polling fallback**:
- Background loop runs every 30 seconds
- Finds all pending transactions older than 2 minutes with no webhook received
- Queries provider directly using `provider_reference`
- Handles cases where webhooks are delayed or dropped

**On confirmation**:
- Validate the confirmed amount matches the expected `amount_ngn` on the transaction
- Validate the currency matches (NGN)
- Update transaction status from `pending` → `processing`
- Record `provider_confirmed_at` timestamp
- Proceed to Stage 2

**On payment not yet received**:
- Leave in pending, check again on next poll cycle
- If pending for more than 30 minutes with no confirmation → mark failed with reason `PAYMENT_TIMEOUT`

### Stage 2: cNGN Transfer on Stellar

Once payment is confirmed, execute the cNGN transfer to the user's wallet:

**Verify trustline**:
- Check wallet still has active cNGN trustline
- If trustline missing: mark failed with reason `TRUSTLINE_NOT_FOUND`, initiate refund

**Check cNGN liquidity**:
- Confirm Aframp's Stellar account holds enough cNGN
- If insufficient: mark failed with reason `INSUFFICIENT_CNGN_BALANCE`, initiate refund

**Build cNGN payment transaction**:
- Use the `amount_cngn` from the transaction record (locked at quote time)
- Set memo to `tx_id` for traceability
- Set fee using current Stellar base fee
- Sign and submit the Stellar transaction
- Store `stellar_tx_hash` on the transaction record
- Update status from `processing` → `awaiting Stellar confirmation`
- Proceed to Stage 3

**Retry logic for Stellar submission**:
- On transient error (network timeout, Stellar overloaded): retry up to 3 times with exponential backoff (2s, 4s, 8s)
- On permanent error (invalid sequence number, bad signature): do not retry, mark failed, initiate refund

### Stage 3: Stellar Confirmation Monitoring

After submitting the cNGN transaction:

- Poll Stellar every 10 seconds for the transaction hash
- Once confirmed (1 ledger close is sufficient for cNGN transfers):
  - Update transaction status from `processing` → `completed`
  - Record `completed_at` timestamp
  - Store final `stellar_tx_hash` and `explorer_url`
- If not confirmed after 5 minutes: re-check submission, consider resubmission

### Stage 4: Failure Handling & Refunds

When a transaction reaches failed status after payment was taken, a refund must be initiated automatically:

**Refund conditions**:
- Payment confirmed by provider BUT cNGN transfer failed (trustline missing, insufficient balance, Stellar error after retries)

**No refund needed when**:
- Transaction failed before payment was confirmed (e.g. `PAYMENT_TIMEOUT`)

**Refund execution**:
- Update transaction status to `failed`
- Create a refund record in the database
- Call Payment Orchestration Service with refund instruction:
  - Flutterwave → Initiate refund via Flutterwave Refund API
  - Paystack → Initiate refund via Paystack Refund API
  - M-Pesa → Initiate B2C reversal
- On successful refund initiation: update transaction status to `refunded`, record `refunded_at`
- On refund failure: mark refund as `pending_manual_review`, alert ops team immediately

## Transaction Status Transitions

```
pending
│
├─ Payment confirmed (webhook or poll) ──────────────────► processing
│
├─ Payment timeout (30 min) ──────────────────────────────► failed (no refund)
│
└─ Payment explicitly failed by provider ─────────────────► failed (no refund)

processing
│
├─ cNGN transfer confirmed on Stellar ───────────────────► completed ✅
│
└─ cNGN transfer failed (all retries exhausted) ─────────► failed
                                                           │
                                                           ├─ Refund initiated ──► refunded ✅
                                                           │
                                                           └─ Refund failed ─────► pending_manual_review 🚨
```

## Database Updates

Every state transition must be recorded atomically:

```sql
-- Payment confirmed
UPDATE transactions SET
  status = 'processing',
  provider_confirmed_at = NOW(),
  updated_at = NOW()
WHERE tx_id = $1 AND status = 'pending';

-- cNGN transfer submitted
UPDATE transactions SET
  stellar_tx_hash = $2,
  updated_at = NOW()
WHERE tx_id = $1 AND status = 'processing';

-- Transaction completed
UPDATE transactions SET
  status = 'completed',
  completed_at = NOW(),
  updated_at = NOW()
WHERE tx_id = $1 AND status = 'processing';

-- Transaction failed
UPDATE transactions SET
  status = 'failed',
  failure_reason = $2,
  failed_at = NOW(),
  updated_at = NOW()
WHERE tx_id = $1;

-- Transaction refunded
UPDATE transactions SET
  status = 'refunded',
  refunded_at = NOW(),
  updated_at = NOW()
WHERE tx_id = $1 AND status = 'failed';
```

**Critical**: All updates use `WHERE status = $current_status` to prevent race conditions between the webhook handler and the polling fallback updating the same transaction simultaneously.

## Configuration

```toml
[processor]
# Polling fallback
poll_interval_secs = 30
pending_timeout_mins = 30

# cNGN Stellar transfer retries
stellar_max_retries = 3
stellar_retry_backoff_secs = [2, 4, 8]

# Stellar confirmation monitoring
stellar_confirmation_poll_secs = 10
stellar_confirmation_timeout_mins = 5

# Refund
refund_max_retries = 3
refund_retry_backoff_secs = [30, 60, 120]
```

## Key Implementation Details

### Optimistic Locking

All status updates use `WHERE status = $current_status` to ensure only one process can transition a transaction from one state to another. This prevents:
- Webhook and polling fallback both updating the same transaction
- Double-processing of the same payment confirmation
- Race conditions in concurrent environments

### SELECT ... FOR UPDATE SKIP LOCKED

When the polling fallback fetches pending transactions, use:
```sql
SELECT * FROM transactions
WHERE status = 'pending'
FOR UPDATE SKIP LOCKED
```

This prevents multiple worker instances from processing the same transaction simultaneously if the service is scaled horizontally.

### Idempotency

Every operation is idempotent:
- Processing the same webhook twice results in the same state (second update fails the WHERE clause)
- Retrying a failed Stellar submission uses the same transaction hash
- Refund retries use the same refund record

### Amount Locking

The `amount_cngn` is locked at quote time and stored on the transaction record. It is **never** inferred at processing time. Rate changes between quote and processing must never affect what the user receives.

### Stellar Hash Logging

The Stellar transaction hash is logged immediately after submission, before awaiting confirmation. If the worker crashes between submission and confirmation, the hash is recoverable from logs.

## Metrics

The processor emits Prometheus metrics at every key stage:

```
# Counters
onramp_payments_confirmed_total{provider}
onramp_payments_failed_total{provider, reason}
onramp_cngn_transfers_submitted_total
onramp_cngn_transfers_confirmed_total
onramp_cngn_transfers_failed_total{reason}
onramp_refunds_initiated_total{provider}
onramp_refunds_completed_total{provider}
onramp_refunds_failed_total{provider}
onramp_manual_reviews_total

# Histograms
onramp_payment_confirmation_duration_seconds{provider}
onramp_cngn_transfer_duration_seconds
onramp_stellar_confirmation_duration_seconds
onramp_total_processing_duration_seconds
```

## Performance Targets

| Operation | Target |
|-----------|--------|
| Webhook → processing status update | < 500ms |
| Payment confirmation → cNGN submitted to Stellar | < 2s |
| Stellar submission → confirmation | < 10s (1 ledger close) |
| End-to-end (webhook → cNGN in wallet) | < 30s |
| Polling fallback cycle | Every 30s |
| Refund initiation after failure | < 60s |

## Critical Failure States

### pending_manual_review 🚨

This is the most critical failure state in the system:
- User has paid NGN
- Received neither cNGN nor a refund
- Refund initiation itself failed

**Action**: Immediate ops alert (Slack/PagerDuty)
- Include transaction ID, user wallet, amount, and error details
- Ops team must manually investigate and resolve

## Testing Checklist

- ✅ Webhook triggers payment confirmation and status updates to processing
- ✅ Polling fallback queries provider for txs with no webhook after 2 min
- ✅ PAYMENT_TIMEOUT failure after 30 minutes with no confirmation
- ✅ Provider failure webhook marks transaction failed with no refund
- ✅ cNGN transfer submitted to Stellar on payment confirmation
- ✅ TRUSTLINE_NOT_FOUND triggers failure and refund
- ✅ INSUFFICIENT_CNGN_BALANCE triggers failure and refund
- ✅ Stellar transient error retries 3 times with backoff
- ✅ Stellar permanent error does not retry, fails immediately
- ✅ Transaction marked completed after Stellar confirmation
- ✅ Refund initiated when cNGN transfer fails after payment taken
- ✅ Transaction marked refunded on successful refund
- ✅ pending_manual_review when refund itself fails
- ✅ Concurrent webhook + poll do not both update the same transaction
- ✅ Optimistic locking prevents double-processing
- ✅ Integration test: webhook → cNGN transfer → completed full flow
- ✅ Integration test: payment confirmed → Stellar failure → refund flow

## Integration Points

The processor integrates with:

1. **Webhook Processing System** (#21)
   - Consumes webhook events from Flutterwave, Paystack, M-Pesa
   - Verifies signatures and parses events

2. **Payment Orchestration Service** (#20)
   - Queries provider for payment status (polling fallback)
   - Initiates refunds on failure

3. **cNGN Payment Transaction Builder** (#11)
   - Builds and signs cNGN transfer transactions
   - Submits to Stellar

4. **Stellar Transaction Monitoring** (#12)
   - Polls for transaction confirmation
   - Retrieves transaction details

5. **Trustline Management** (#10)
   - Verifies trustline exists before transfer
   - Checks trustline authorization

6. **Database** (#6)
   - Stores transaction state
   - Records webhook events
   - Tracks refund status

7. **Redis** (#7)
   - Caches provider health status
   - Stores temporary processing state

## Deployment

The processor is registered in `src/main.rs` and spawned as a tokio task on startup:

```rust
let processor = OnrampProcessor::new(
    Arc::new(db_pool),
    Arc::new(stellar_client),
    Arc::new(payment_orchestrator),
    OnrampProcessorConfig::from_env(),
);

tokio::spawn(processor.run(worker_shutdown_rx));
```

Enable/disable via environment variable:
```bash
ONRAMP_PROCESSOR_ENABLED=true
```

## Monitoring & Alerting

Monitor these metrics in production:

1. **Payment Confirmation Rate**
   - `onramp_payments_confirmed_total` should increase steadily
   - Alert if rate drops below threshold

2. **Failure Rate**
   - `onramp_payments_failed_total` and `onramp_cngn_transfers_failed_total`
   - Alert if failure rate exceeds 5%

3. **Manual Review Queue**
   - `onramp_manual_reviews_total` should be near zero
   - Alert if any transactions enter this state

4. **Processing Duration**
   - `onramp_total_processing_duration_seconds` should be < 30s p99
   - Alert if p99 exceeds 60s

5. **Refund Success Rate**
   - `onramp_refunds_completed_total` / `onramp_refunds_initiated_total`
   - Alert if success rate < 99%

## Notes

- This worker is the highest-stakes code in the entire codebase — it moves real money
- Every state transition must be idempotent and race-condition proof
- Use `SELECT ... FOR UPDATE SKIP LOCKED` when fetching pending transactions
- The `pending_manual_review` state must trigger an immediate ops alert
- Never infer `amount_cngn` at processing time — always use the value locked at quote time
- Log every Stellar transaction hash immediately after submission
- Consider implementing a dead letter queue for transactions stuck in processing for more than 10 minutes

## Success Criteria

✅ Every confirmed NGN payment results in cNGN arriving in the user's wallet automatically
✅ Every failure after payment is taken results in an automatic refund — no user left empty handed
✅ No transaction is processed twice — optimistic locking and idempotency throughout
✅ Ops team is alerted immediately for any case requiring manual intervention
✅ Full audit trail on every transaction from initiation to completion
