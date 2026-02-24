# Fees API Quick Start Guide

## Overview
The `/api/fees` endpoint provides transparent access to Aframp's fee structure, enabling users to understand costs before initiating transactions.

## Base URL
```
http://localhost:8000/api/fees
```

## Quick Examples

### 1. Get Full Fee Structure
See all fees for all transaction types and providers:

```bash
curl http://localhost:8000/api/fees
```

**Use Case:** Display fee information on pricing page or help section.

### 2. Calculate Fees for Specific Transaction
Calculate exact fees for a 50,000 NGN onramp via Flutterwave:

```bash
curl "http://localhost:8000/api/fees?amount=50000&type=onramp&provider=flutterwave"
```

**Use Case:** Show user exact fees before they confirm a transaction.

### 3. Compare Providers
Find the cheapest provider for a 50,000 NGN onramp:

```bash
curl "http://localhost:8000/api/fees?amount=50000&type=onramp"
```

**Use Case:** Help users choose the most cost-effective payment method.

## Transaction Types

| Type | Description |
|------|-------------|
| `onramp` | Converting fiat (NGN) to crypto (CNGN) |
| `offramp` | Converting crypto (CNGN) to fiat (NGN) |
| `bill_payment` | Paying bills using crypto |

## Supported Providers

| Provider | Description |
|----------|-------------|
| `flutterwave` | Flutterwave payment gateway |
| `paystack` | Paystack payment gateway |
| `mpesa` | M-Pesa mobile money |

## Response Examples

### Full Structure Response
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
        }
      }
    }
  },
  "timestamp": "2026-02-24T10:30:00Z"
}
```

### Calculated Fees Response
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

### Provider Comparison Response
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
    }
  ],
  "cheapest_provider": "flutterwave",
  "timestamp": "2026-02-24T10:30:00Z"
}
```

## Error Responses

### Invalid Transaction Type
```json
{
  "error": {
    "code": "INVALID_TYPE",
    "message": "Transaction type 'xyz' is not supported.",
    "supported_types": ["onramp", "offramp", "bill_payment"]
  }
}
```

### Invalid Provider
```json
{
  "error": {
    "code": "INVALID_PROVIDER",
    "message": "Provider 'xyz' is not supported.",
    "supported_providers": ["flutterwave", "paystack", "mpesa"]
  }
}
```

### Invalid Amount
```json
{
  "error": {
    "code": "INVALID_AMOUNT",
    "message": "Amount must be a positive number greater than 0."
  }
}
```

### Missing Type Parameter
```json
{
  "error": {
    "code": "MISSING_TYPE",
    "message": "Query param 'type' is required when 'amount' is provided.",
    "supported_types": ["onramp", "offramp", "bill_payment"]
  }
}
```

## Frontend Integration Examples

### JavaScript/TypeScript
```typescript
// Get full fee structure
async function getFeeStructure() {
  const response = await fetch('http://localhost:8000/api/fees');
  const data = await response.json();
  return data.fee_structure;
}

// Calculate fees for specific transaction
async function calculateFees(amount: number, type: string, provider: string) {
  const url = `http://localhost:8000/api/fees?amount=${amount}&type=${type}&provider=${provider}`;
  const response = await fetch(url);
  const data = await response.json();
  return data.breakdown;
}

// Compare providers
async function compareProviders(amount: number, type: string) {
  const url = `http://localhost:8000/api/fees?amount=${amount}&type=${type}`;
  const response = await fetch(url);
  const data = await response.json();
  return {
    comparison: data.comparison,
    cheapest: data.cheapest_provider
  };
}
```

### React Hook Example
```typescript
import { useState, useEffect } from 'react';

function useFeeCalculation(amount: number, type: string, provider: string) {
  const [fees, setFees] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);

  useEffect(() => {
    if (!amount || !type || !provider) return;

    setLoading(true);
    fetch(`http://localhost:8000/api/fees?amount=${amount}&type=${type}&provider=${provider}`)
      .then(res => res.json())
      .then(data => {
        setFees(data.breakdown);
        setLoading(false);
      })
      .catch(err => {
        setError(err);
        setLoading(false);
      });
  }, [amount, type, provider]);

  return { fees, loading, error };
}
```

## Performance Notes

- **Caching**: Responses are cached in Redis
  - Full structure: 5 minutes
  - Calculated fees: 1 minute
  - Provider comparison: 1 minute
- **Response Time**: Typically < 50ms (cached) or < 200ms (uncached)
- **Rate Limiting**: None currently (consider adding for production)

## Testing

### Manual Testing with curl
```bash
# Test full structure
curl -i http://localhost:8000/api/fees

# Test calculation
curl -i "http://localhost:8000/api/fees?amount=10000&type=onramp&provider=flutterwave"

# Test comparison
curl -i "http://localhost:8000/api/fees?amount=10000&type=onramp"

# Test validation errors
curl -i "http://localhost:8000/api/fees?amount=10000&type=invalid"
curl -i "http://localhost:8000/api/fees?amount=-100&type=onramp"
curl -i "http://localhost:8000/api/fees?amount=10000"
```

### Integration Tests
```bash
# Run integration tests (requires test database)
export DATABASE_URL="postgresql://postgres:postgres@localhost/aframp_test"
cargo test fees_api_test --features database
```

## Common Use Cases

### 1. Transaction Preview
Show users the exact fees before they confirm:
```
GET /api/fees?amount=50000&type=onramp&provider=flutterwave
```

### 2. Provider Selection
Help users choose the cheapest option:
```
GET /api/fees?amount=50000&type=onramp
```

### 3. Pricing Page
Display fee structure on marketing/help pages:
```
GET /api/fees
```

### 4. Fee Calculator Widget
Build an interactive calculator:
```javascript
// User inputs amount and type
// Fetch comparison
// Display all options with fees
// Highlight cheapest provider
```

## Troubleshooting

### Endpoint Returns 503
- Check database connection
- Verify fee_structures table has data
- Check application logs

### Fees Seem Incorrect
- Verify fee_structures table configuration
- Check for active fee tiers
- Ensure amount falls within configured ranges

### Cache Not Working
- Verify Redis connection
- Check REDIS_URL environment variable
- Review cache configuration in logs

## Next Steps

1. **Frontend Integration**: Use the endpoint in your UI
2. **Monitoring**: Set up alerts for endpoint errors
3. **Documentation**: Share with frontend team
4. **Testing**: Add to your test suite
5. **Production**: Deploy and monitor performance
