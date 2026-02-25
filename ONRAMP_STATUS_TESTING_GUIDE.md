# Onramp Status Endpoint Testing Guide

## Quick Start

### Prerequisites
1. Backend server running on `http://localhost:8000`
2. Database with transaction records
3. Redis cache available
4. Payment providers configured (Flutterwave, Paystack, or M-Pesa)
5. Stellar client configured (testnet or mainnet)

### Basic Test

```bash
# Test with a valid transaction ID
curl http://localhost:8000/api/onramp/status/tx_01J2KXXXXXXXXXXXXXXXXXX

# Test with invalid transaction ID (should return 404)
curl http://localhost:8000/api/onramp/status/invalid-tx-id
```

## Test Scenarios

### 1. Transaction Not Found (404)

**Request:**
```bash
curl -i http://localhost:8000/api/onramp/status/00000000-0000-0000-0000-000000000000
```

**Expected Response:**
```json
{
  "error": {
    "code": "TRANSACTION_NOT_FOUND",
    "message": "Transaction '00000000-0000-0000-0000-000000000000' not found",
    "tx_id": "00000000-0000-0000-0000-000000000000"
  }
}
```

**Status Code:** 404

### 2. Pending Transaction

**Request:**
```bash
curl -i http://localhost:8000/api/onramp/status/{pending_tx_id}
```

**Expected Response:**
```json
{
  "tx_id": "...",
  "status": "pending",
  "stage": "awaiting_payment",
  "message": "Waiting for your payment to be confirmed by ...",
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
    "wallet_address": "G...",
    "chain": "stellar",
    "created_at": "2026-02-18T10:31:00Z",
    "updated_at": "2026-02-18T10:31:00Z"
  },
  "provider_status": {
    "confirmed": false,
    "reference": "FLW-...",
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

**Status Code:** 200

**Validation:**
- `status` is "pending"
- `stage` is "awaiting_payment"
- `provider_status` is present
- `blockchain` is null
- `timeline` has 1 entry

### 3. Processing Transaction

**Request:**
```bash
curl -i http://localhost:8000/api/onramp/status/{processing_tx_id}
```

**Expected Response:**
```json
{
  "tx_id": "...",
  "status": "processing",
  "stage": "sending_cngn",
  "message": "Payment confirmed. Sending 49,125 cNGN to your wallet.",
  "transaction": { ... },
  "provider_status": {
    "confirmed": true,
    "reference": "FLW-...",
    "checked_at": "2026-02-18T10:33:00Z"
  },
  "blockchain": {
    "stellar_tx_hash": "pending",
    "confirmations": 0,
    "confirmed": false,
    "checked_at": "2026-02-18T10:33:10Z"
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
    }
  ]
}
```

**Status Code:** 200

**Validation:**
- `status` is "processing"
- `stage` is "sending_cngn"
- `provider_status.confirmed` is true
- `blockchain` is present
- `timeline` has 2 entries

### 4. Completed Transaction

**Request:**
```bash
curl -i http://localhost:8000/api/onramp/status/{completed_tx_id}
```

**Expected Response:**
```json
{
  "tx_id": "...",
  "status": "completed",
  "stage": "done",
  "message": "49,125 cNGN has been sent to your wallet successfully.",
  "transaction": {
    ...
    "completed_at": "2026-02-18T10:34:30Z"
  },
  "provider_status": {
    "confirmed": true,
    "reference": "FLW-...",
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

**Status Code:** 200

**Validation:**
- `status` is "completed"
- `stage` is "done"
- `transaction.completed_at` is present
- `blockchain.confirmed` is true
- `blockchain.explorer_url` is present
- `timeline` has 3 entries

### 5. Failed Transaction

**Request:**
```bash
curl -i http://localhost:8000/api/onramp/status/{failed_tx_id}
```

**Expected Response:**
```json
{
  "tx_id": "...",
  "status": "failed",
  "stage": "failed",
  "message": "Transaction failed. If any payment was taken, a refund will be initiated automatically.",
  "failure_reason": "PAYMENT_TIMEOUT",
  "transaction": { ... },
  "provider_status": {
    "confirmed": false,
    "reference": "FLW-...",
    "checked_at": "2026-02-18T10:40:00Z"
  },
  "timeline": [
    {
      "status": "pending",
      "timestamp": "2026-02-18T10:31:00Z",
      "note": "Transaction initiated"
    },
    {
      "status": "failed",
      "timestamp": "2026-02-18T10:40:00Z",
      "note": "Payment timed out"
    }
  ]
}
```

**Status Code:** 200

**Validation:**
- `status` is "failed"
- `stage` is "failed"
- `failure_reason` is present
- `blockchain` is null
- `timeline` has 2 entries

### 6. Refunded Transaction

**Request:**
```bash
curl -i http://localhost:8000/api/onramp/status/{refunded_tx_id}
```

**Expected Response:**
```json
{
  "tx_id": "...",
  "status": "refunded",
  "stage": "refunded",
  "message": "Transaction was refunded successfully.",
  "transaction": { ... },
  "timeline": [
    {
      "status": "pending",
      "timestamp": "2026-02-18T10:31:00Z",
      "note": "Transaction initiated"
    },
    {
      "status": "refunded",
      "timestamp": "2026-02-18T10:45:00Z",
      "note": "Refund processed"
    }
  ]
}
```

**Status Code:** 200

**Validation:**
- `status` is "refunded"
- `stage` is "refunded"
- `timeline` has 2 entries

## Performance Testing

### Cache Hit Test

**First Request (Cache Miss):**
```bash
time curl http://localhost:8000/api/onramp/status/{tx_id}
```

**Second Request (Cache Hit):**
```bash
time curl http://localhost:8000/api/onramp/status/{tx_id}
```

**Expected:**
- First request: 100-500ms (depending on status)
- Second request: < 20ms (cache hit)

### Cache TTL Verification

**Test Pending Transaction Cache:**
```bash
# First request
curl http://localhost:8000/api/onramp/status/{pending_tx_id}

# Wait 6 seconds (TTL is 5 seconds)
sleep 6

# Second request should be cache miss
curl http://localhost:8000/api/onramp/status/{pending_tx_id}
```

**Test Completed Transaction Cache:**
```bash
# First request
curl http://localhost:8000/api/onramp/status/{completed_tx_id}

# Wait 6 seconds
sleep 6

# Second request should still be cache hit (TTL is 300 seconds)
curl http://localhost:8000/api/onramp/status/{completed_tx_id}
```

## Load Testing

### Using Apache Bench

```bash
# Test 1000 requests with 10 concurrent connections
ab -n 1000 -c 10 http://localhost:8000/api/onramp/status/{tx_id}
```

**Expected Results:**
- Requests per second: > 100
- Mean response time: < 100ms (for cached responses)
- Failed requests: 0

### Using wrk

```bash
# Test for 30 seconds with 10 threads and 100 connections
wrk -t10 -c100 -d30s http://localhost:8000/api/onramp/status/{tx_id}
```

## Integration Test Flow

### Complete Onramp Journey

```bash
# 1. Create a quote
QUOTE_RESPONSE=$(curl -X POST http://localhost:8000/api/onramp/quote \
  -H "Content-Type: application/json" \
  -d '{
    "amount_ngn": 50000,
    "wallet_address": "GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX",
    "provider": "flutterwave"
  }')

QUOTE_ID=$(echo $QUOTE_RESPONSE | jq -r '.quote_id')

# 2. Initiate transaction (assuming this endpoint exists)
TX_RESPONSE=$(curl -X POST http://localhost:8000/api/onramp/initiate \
  -H "Content-Type: application/json" \
  -d "{
    \"quote_id\": \"$QUOTE_ID\"
  }")

TX_ID=$(echo $TX_RESPONSE | jq -r '.tx_id')

# 3. Poll status until completed
while true; do
  STATUS=$(curl -s http://localhost:8000/api/onramp/status/$TX_ID | jq -r '.status')
  echo "Status: $STATUS"
  
  if [ "$STATUS" = "completed" ] || [ "$STATUS" = "failed" ] || [ "$STATUS" = "refunded" ]; then
    break
  fi
  
  sleep 3
done

# 4. Get final status
curl http://localhost:8000/api/onramp/status/$TX_ID | jq
```

## Monitoring & Debugging

### Check Redis Cache

```bash
# Connect to Redis
redis-cli

# Check if transaction is cached
GET api:onramp:status:{tx_id}

# Check TTL
TTL api:onramp:status:{tx_id}

# Clear cache for testing
DEL api:onramp:status:{tx_id}
```

### Check Database

```sql
-- View transaction record
SELECT * FROM transactions WHERE transaction_id = '{tx_id}';

-- View all pending transactions
SELECT transaction_id, status, created_at, updated_at 
FROM transactions 
WHERE status = 'pending' 
ORDER BY created_at DESC;
```

### Check Logs

```bash
# Follow application logs
tail -f /var/log/aframp/app.log | grep "onramp/status"

# Search for specific transaction
grep "{tx_id}" /var/log/aframp/app.log
```

## Common Issues & Solutions

### Issue: 404 for Valid Transaction
**Cause:** Transaction doesn't exist in database
**Solution:** Verify transaction was created successfully

### Issue: Provider Status Always Null
**Cause:** Payment provider not configured or unreachable
**Solution:** Check payment provider credentials and network connectivity

### Issue: Blockchain Status Always Null
**Cause:** Stellar client not configured or Horizon unreachable
**Solution:** Check Stellar configuration and Horizon URL

### Issue: Slow Response Times
**Cause:** Cache not working or external services slow
**Solution:** 
- Verify Redis is running
- Check payment provider response times
- Check Stellar Horizon response times

### Issue: Cache Not Expiring
**Cause:** Redis TTL not working
**Solution:** Verify Redis configuration supports TTL

## Acceptance Criteria Checklist

- [x] GET /api/onramp/status/:tx_id endpoint implemented
- [x] Returns 404 for unknown tx_id
- [x] Returns correct status for all states (pending, processing, completed, failed, refunded)
- [x] Queries payment provider for live confirmation when status is pending
- [x] Queries Stellar for blockchain confirmation when status is processing
- [x] Skips live checks for terminal states
- [x] Returns provider_status block with confirmation flag and reference
- [x] Returns blockchain block with stellar_tx_hash and confirmations
- [x] Returns timeline array showing full status history
- [x] Returns explorer_url in blockchain block when confirmed
- [x] Caches pending responses for 5 seconds
- [x] Caches processing responses for 10 seconds
- [x] Caches terminal state responses for 300 seconds
- [x] No authentication required
- [ ] Response time < 20ms on cache hit (requires performance testing)
- [ ] Response time < 200ms on cache miss for terminal states (requires performance testing)
- [ ] Response time < 500ms on cache miss for live-checked states (requires performance testing)

## Next Steps

1. Run integration tests with real payment providers
2. Perform load testing to verify performance targets
3. Monitor cache hit rates in production
4. Set up alerts for high error rates
5. Document any edge cases discovered during testing
