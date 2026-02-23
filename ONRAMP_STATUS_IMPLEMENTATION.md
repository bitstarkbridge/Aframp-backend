# Onramp Status Endpoint Implementation

## Overview

This document describes the implementation of the `GET /api/onramp/status/:tx_id` endpoint as specified in Issue #52.

## Implementation Summary

### Files Created/Modified

1. **src/api/onramp.rs** (NEW)
   - Complete implementation of the onramp status endpoint
   - Service layer with caching, provider checks, and blockchain verification
   - Response structures matching the API specification

2. **src/api/mod.rs** (MODIFIED)
   - Added `pub mod onramp;` to expose the onramp API module

3. **src/cache/keys.rs** (MODIFIED)
   - Added `StatusKey` structure for onramp status caching
   - Cache key format: `api:onramp:status:{tx_id}`

4. **src/main.rs** (MODIFIED)
   - Integrated onramp status service into the application router
   - Added route: `GET /api/onramp/status/:tx_id`
   - Initialized dependencies: transaction repository, payment factory, stellar client, cache

5. **tests/onramp_status_test.rs** (NEW)
   - Unit tests for key functionality
   - Validates serialization, cache keys, TTL values, and message formats

## Architecture

### Service Layer: `OnrampStatusService`

The service encapsulates all business logic for status retrieval:

```rust
pub struct OnrampStatusService {
    pub transaction_repo: Arc<TransactionRepository>,
    pub cache: Arc<RedisCache>,
    pub stellar_client: Arc<StellarClient>,
    pub payment_factory: Arc<PaymentProviderFactory>,
}
```

### Key Methods

1. **`get_status(tx_id)`** - Main entry point
   - Checks cache first
   - Fetches transaction from database
   - Enriches with provider/blockchain status
   - Caches response with appropriate TTL
   - Returns unified response

2. **`check_provider_status(provider, reference)`**
   - Queries payment provider for confirmation
   - Returns `ProviderStatus` with confirmation flag
   - Handles provider errors gracefully

3. **`check_blockchain_status(tx_hash)`**
   - Queries Stellar Horizon for transaction confirmation
   - Returns `BlockchainStatus` with confirmations and explorer URL
   - Handles blockchain errors gracefully

4. **`build_timeline(status, created_at, updated_at, metadata)`**
   - Constructs transaction history from status transitions
   - Returns chronological timeline entries

## Response Structures

### OnrampStatusResponse

```rust
pub struct OnrampStatusResponse {
    pub tx_id: String,
    pub status: String,
    pub stage: TransactionStage,
    pub message: String,
    pub failure_reason: Option<String>,
    pub transaction: TransactionDetail,
    pub provider_status: Option<ProviderStatus>,
    pub blockchain: Option<BlockchainStatus>,
    pub timeline: Vec<TimelineEntry>,
}
```

### TransactionStage

```rust
pub enum TransactionStage {
    AwaitingPayment,  // pending
    SendingCngn,      // processing
    Done,             // completed
    Failed,           // failed
    Refunded,         // refunded
}
```

## Caching Strategy

| Status      | Cache TTL | Reason                              |
|-------------|-----------|-------------------------------------|
| pending     | 5 sec     | Changes frequently during payment   |
| processing  | 10 sec    | Blockchain confirmation in progress |
| completed   | 300 sec   | Terminal state - won't change       |
| failed      | 300 sec   | Terminal state - won't change       |
| refunded    | 300 sec   | Terminal state - won't change       |

Cache key format: `api:onramp:status:{tx_id}`

## Status Enrichment Logic

### For `pending` Status
- Query payment provider for live confirmation status
- If provider confirms payment but DB still shows pending, response reflects this enrichment
- Provider status includes: `confirmed`, `reference`, `checked_at`

### For `processing` Status
- Query Stellar blockchain for transaction confirmation
- Check if transaction hash exists and is confirmed
- Blockchain status includes: `stellar_tx_hash`, `confirmations`, `confirmed`, `explorer_url`, `checked_at`

### For Terminal States (`completed`, `failed`, `refunded`)
- Skip live checks - serve from database only
- Include historical provider/blockchain data from transaction record

## Error Handling

### 404 - Transaction Not Found
```json
{
  "error": {
    "code": "TRANSACTION_NOT_FOUND",
    "message": "Transaction 'tx_01J2K...' not found",
    "tx_id": "tx_01J2K..."
  }
}
```

### 503 - Service Unavailable
```json
{
  "error": {
    "code": "SERVICE_UNAVAILABLE",
    "message": "Status service is temporarily unavailable. Please try again.",
    "retry_after": 10
  }
}
```

## Timeline Construction

The timeline shows the full transaction journey:

1. **pending** → "Transaction initiated" (created_at)
2. **processing** → "Payment confirmed" (updated_at)
3. **completed** → "cNGN sent on Stellar" (updated_at)
4. **failed** → Failure reason from metadata (updated_at)
5. **refunded** → "Refund processed" (updated_at)

## Integration Points

### Database
- Uses `TransactionRepository` to fetch transaction records
- Reads from `transactions` table
- No writes - read-only endpoint

### Cache (Redis)
- Checks cache before database query
- Stores responses with status-appropriate TTL
- Cache misses trigger full enrichment flow

### Payment Providers
- Uses `PaymentProviderFactory` to get provider instances
- Calls `get_payment_status()` for pending transactions
- Supports Flutterwave, Paystack, M-Pesa

### Stellar Blockchain
- Uses `StellarClient` to query transaction confirmations
- Calls `get_transaction_by_hash()` for processing transactions
- Generates explorer URLs based on network (testnet/mainnet)

## Performance Characteristics

### Cache Hit (any status)
- Target: < 20ms
- No database or external calls
- Direct Redis lookup

### Cache Miss - Terminal State
- Target: < 100ms
- Single database query
- No provider/blockchain checks

### Cache Miss - Pending
- Target: < 400ms
- Database query + provider API call
- Provider timeout: ~200ms

### Cache Miss - Processing
- Target: < 500ms
- Database query + Stellar API call
- Stellar timeout: ~300ms

## Security Considerations

### No Authentication Required
- `tx_id` is a UUID - unguessable by design
- Possession of `tx_id` is sufficient access control
- Read-only operation - no state changes

### Rate Limiting
- Should be applied at reverse proxy/API gateway level
- Recommended: 100 requests/minute per IP

## Testing Checklist

- [x] Response structures defined
- [x] Service layer implemented
- [x] Cache integration complete
- [x] Provider status check implemented
- [x] Blockchain status check implemented
- [x] Timeline construction implemented
- [x] Error handling implemented
- [x] Route registered in main.rs
- [x] Unit tests created
- [ ] Integration tests (requires running services)
- [ ] Load testing (requires deployed environment)

## Future Enhancements

1. **Poll Interval Hint**
   - Add `poll_interval_ms` field to response
   - Tell frontend how often to poll based on status
   - Example: 3000ms for pending, 5000ms for processing

2. **Webhook Alternative**
   - Implement webhook notifications for status changes
   - Reduce polling load on the API

3. **Status History**
   - Store full status transition history in database
   - Provide richer timeline with all intermediate states

4. **Metrics & Monitoring**
   - Track cache hit rates
   - Monitor response times by status
   - Alert on high error rates

## API Usage Example

### Request
```bash
GET /api/onramp/status/tx_01J2KXXXXXXXXXXXXXXXXXX
```

### Response (Pending)
```json
{
  "tx_id": "tx_01J2KXXXXXXXXXXXXXXXXXX",
  "status": "pending",
  "stage": "awaiting_payment",
  "message": "Waiting for your payment to be confirmed by Flutterwave.",
  "transaction": {
    "type": "onramp",
    "amount_ngn": 50000,
    "amount_cngn": 49125,
    "fees": {
      "platform_fee_ngn": 500,
      "provider_fee_ngn": 375,
      "total_fee_ngn": 875
    },
    "provider": "flutterwave",
    "wallet_address": "GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX",
    "chain": "stellar",
    "created_at": "2026-02-18T10:31:00Z",
    "updated_at": "2026-02-18T10:31:00Z"
  },
  "provider_status": {
    "confirmed": false,
    "reference": "FLW-XXXXXXXXXXXX",
    "checked_at": "2026-02-18T10:32:00Z"
  },
  "timeline": [
    {
      "status": "pending",
      "timestamp": "2026-02-18T10:31:00Z",
      "note": "Transaction initiated"
    }
  ]
}
```

### Response (Completed)
```json
{
  "tx_id": "tx_01J2KXXXXXXXXXXXXXXXXXX",
  "status": "completed",
  "stage": "done",
  "message": "49,125 cNGN has been sent to your wallet successfully.",
  "transaction": {
    "type": "onramp",
    "amount_ngn": 50000,
    "amount_cngn": 49125,
    "fees": {
      "platform_fee_ngn": 500,
      "provider_fee_ngn": 375,
      "total_fee_ngn": 875
    },
    "provider": "flutterwave",
    "wallet_address": "GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX",
    "chain": "stellar",
    "created_at": "2026-02-18T10:31:00Z",
    "updated_at": "2026-02-18T10:34:30Z",
    "completed_at": "2026-02-18T10:34:30Z"
  },
  "provider_status": {
    "confirmed": true,
    "reference": "FLW-XXXXXXXXXXXX",
    "checked_at": "2026-02-18T10:33:00Z"
  },
  "blockchain": {
    "stellar_tx_hash": "a1b2c3d4e5f6...",
    "confirmations": 1,
    "confirmed": true,
    "explorer_url": "https://stellar.expert/explorer/public/tx/a1b2c3d4e5f6",
    "checked_at": "2026-02-18T10:34:30Z"
  },
  "timeline": [
    {
      "status": "pending",
      "timestamp": "2026-02-18T10:31:00Z",
      "note": "Transaction initiated"
    },
    {
      "status": "processing",
      "timestamp": "2026-02-18T10:33:00Z",
      "note": "Payment confirmed"
    },
    {
      "status": "completed",
      "timestamp": "2026-02-18T10:34:30Z",
      "note": "cNGN sent on Stellar"
    }
  ]
}
```

## Dependencies

- **axum**: Web framework for routing and handlers
- **serde**: Serialization/deserialization
- **chrono**: DateTime handling
- **uuid**: Transaction ID parsing
- **tracing**: Logging and instrumentation
- **sqlx**: Database access
- **redis**: Caching layer
- **stellar-sdk**: Blockchain queries

## Deployment Notes

### Environment Variables
- `DATABASE_URL`: PostgreSQL connection string
- `REDIS_URL`: Redis connection string
- `STELLAR_NETWORK`: "testnet" or "mainnet"
- `STELLAR_HORIZON_URL`: Optional Horizon URL override
- Payment provider credentials (per provider)

### Database Requirements
- `transactions` table must exist with proper schema
- Indexes on `transaction_id` for fast lookups

### Redis Requirements
- Redis 5.0+ recommended
- Sufficient memory for caching responses
- TTL support enabled

## Conclusion

The onramp status endpoint is now fully implemented and ready for integration testing. The implementation follows the specification closely, with proper caching, error handling, and performance optimizations. The endpoint provides a single source of truth for transaction status, enabling the frontend to render appropriate UI states throughout the onramp journey.
