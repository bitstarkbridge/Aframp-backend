# Fee Structure Endpoint Implementation

## Overview
This document describes the implementation of the `/api/fees` endpoint that exposes Aframp's fee structure to clients.

## Implementation Status: ✅ COMPLETE

The fee structure endpoint has been fully implemented and integrated into the application.

## What Was Done

### 1. Endpoint Implementation (`src/api/fees.rs`)
The endpoint was already implemented with the following features:

- **GET /api/fees** - Returns full fee structure for all transaction types and providers
- **GET /api/fees?amount=X&type=Y** - Returns fee comparison across all providers
- **GET /api/fees?amount=X&type=Y&provider=Z** - Returns calculated fees for specific provider

### 2. Route Registration (`src/main.rs`)
Added the fees routes to the main application router:

```rust
// Setup fees API routes with fee calculation service
let fees_routes = if let Some(pool) = db_pool.clone() {
    use services::fee_calculation::FeeCalculationService;
    
    let fee_service = std::sync::Arc::new(FeeCalculationService::new(pool.clone()));
    
    let fees_state = api::fees::FeesState {
        fee_service,
        cache: redis_cache.clone(),
    };
    
    Router::new()
        .route("/api/fees", get(api::fees::get_fees))
        .with_state(fees_state)
} else {
    info!("⏭️  Skipping fees routes (no database)");
    Router::new()
};
```

The route is merged into the main application router alongside other API routes.

## Features

### 1. Full Fee Structure
**Request:** `GET /api/fees`

Returns the complete fee structure for all transaction types (onramp, offramp, bill_payment) and all providers (flutterwave, paystack, mpesa).

**Response:**
```json
{
  "fee_structure": {
    "onramp": {
      "platform_fee_pct": 1.0,
      "min_fee_ngn": 50,
      "max_fee_ngn": 10000,
      "providers": {
        "flutterwave": {
          "fee_pct": 1.4,
          "flat_fee_ngn": 100
        },
        "paystack": {
          "fee_pct": 1.5,
          "flat_fee_ngn": 0
        },
        "mpesa": {
          "fee_pct": 1.0,
          "flat_fee_ngn": 50
        }
      }
    },
    "offramp": { ... },
    "bill_payment": { ... }
  },
  "timestamp": "2026-02-24T10:30:00Z"
}
```

### 2. Fee Calculation for Specific Amount and Provider
**Request:** `GET /api/fees?amount=50000&type=onramp&provider=flutterwave`

Returns calculated fees for a specific amount, transaction type, and provider.

**Response:**
```json
{
  "amount": 50000,
  "type": "onramp",
  "provider": "flutterwave",
  "breakdown": {
    "platform_fee_ngn": 500,
    "provider_fee_ngn": 375,
    "total_fee_ngn": 875,
    "amount_after_fees_ngn": 49125,
    "platform_fee_pct": 1.0,
    "provider_fee_pct": 0.75
  },
  "timestamp": "2026-02-24T10:30:00Z"
}
```

### 3. Provider Comparison
**Request:** `GET /api/fees?amount=50000&type=onramp`

Returns fee comparison across all available providers for the given amount and transaction type.

**Response:**
```json
{
  "amount": 50000,
  "type": "onramp",
  "comparison": [
    {
      "provider": "flutterwave",
      "platform_fee_ngn": 500,
      "provider_fee_ngn": 375,
      "total_fee_ngn": 875,
      "amount_after_fees_ngn": 49125
    },
    {
      "provider": "paystack",
      "platform_fee_ngn": 500,
      "provider_fee_ngn": 500,
      "total_fee_ngn": 1000,
      "amount_after_fees_ngn": 49000
    },
    {
      "provider": "mpesa",
      "platform_fee_ngn": 500,
      "provider_fee_ngn": 450,
      "total_fee_ngn": 950,
      "amount_after_fees_ngn": 49050
    }
  ],
  "cheapest_provider": "flutterwave",
  "timestamp": "2026-02-24T10:30:00Z"
}
```

## Validation

The endpoint includes comprehensive validation:

1. **Type Validation**: Only accepts `onramp`, `offramp`, or `bill_payment`
2. **Provider Validation**: Only accepts `flutterwave`, `paystack`, or `mpesa`
3. **Amount Validation**: Must be a positive number greater than 0
4. **Parameter Combination**: Requires `type` when `amount` is provided

**Error Response Example:**
```json
{
  "error": {
    "code": "INVALID_TYPE",
    "message": "Transaction type 'xyz' is not supported.",
    "supported_types": ["onramp", "offramp", "bill_payment"]
  }
}
```

## Caching

The endpoint implements Redis caching with appropriate TTLs:

- **Full structure**: 300 seconds (5 minutes)
- **Calculated fees**: 60 seconds (1 minute)
- **Provider comparison**: 60 seconds (1 minute)

Cache keys follow the pattern:
- `api:fees:all` - Full structure
- `api:fees:{type}:{provider}:{amount}` - Specific calculation
- `api:fees:{type}:all:{amount}` - Provider comparison

## Dependencies

The implementation relies on:

1. **Fee Calculation Service** (`src/services/fee_calculation.rs`) - Core fee calculation logic
2. **Fee Structure Repository** (`src/database/fee_structure_repository.rs`) - Database access
3. **Redis Cache** (`src/cache/`) - Response caching
4. **Database Schema** - `fee_structures` table with tiered fee configuration

## Testing

Integration tests are available in `tests/fees_api_test.rs` covering:

- Full structure retrieval
- Fee calculation for specific provider
- Provider comparison
- Validation error cases
- Cache behavior

To run tests:
```bash
# Set up test database
export DATABASE_URL="postgresql://postgres:postgres@localhost/aframp_test"

# Run tests
cargo test fees_api_test --features database
```

## API Documentation

The endpoint is now available at:
- **Base URL**: `http://localhost:8000/api/fees`
- **Method**: GET
- **Authentication**: None (public endpoint)

### Query Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| amount | number | No | Transaction amount in NGN |
| type | string | Conditional | Transaction type (required if amount is provided) |
| provider | string | No | Payment provider name |

### Response Codes

| Code | Description |
|------|-------------|
| 200 | Success |
| 400 | Invalid parameters |
| 503 | Fee service unavailable |

## Next Steps

1. **Deploy to staging** - Test the endpoint in staging environment
2. **Frontend integration** - Update frontend to consume the new endpoint
3. **Monitoring** - Add metrics for endpoint usage and cache hit rates
4. **Documentation** - Update API documentation with examples
5. **Load testing** - Verify performance under load

## Notes

- The endpoint is read-only and does not modify any data
- Fees are sourced from the database and calculated dynamically
- The implementation supports tiered fee structures based on amount ranges
- Provider-specific fees are configurable per transaction type and payment method
