mod chains;

use chains::stellar::client::StellarClient;
use chains::stellar::config::StellarConfig;
use tracing::{error, info};
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("Starting Aframp backend service");

    let stellar_config = StellarConfig::from_env()
        .map_err(|e| {
            error!("Failed to load Stellar configuration: {}", e);
            e
        })?;

    let stellar_client = StellarClient::new(stellar_config)
        .map_err(|e| {
            error!("Failed to initialize Stellar client: {}", e);
            e
        })?;

    info!("Stellar client initialized successfully");

    let health_status = stellar_client.health_check().await?;
    if health_status.is_healthy {
        info!(
            "Stellar Horizon is healthy - Response time: {}ms",
            health_status.response_time_ms
        );
    } else {
        error!(
            "Stellar Horizon health check failed: {}",
            health_status.error_message.unwrap_or_else(|| "Unknown error".to_string())
        );
    }

    // Demo functionality
    info!("=== Demo: Testing Stellar functionality ===");
    // Use a properly formatted 56-character Stellar address (this may not exist, but tests validation)
    let test_address = "GD5DJQDQKNR7DSXJVNJTV3P5JJH4KJVTI2JZNYUYIIKHTDNJQXECM4JQ";
    
    match stellar_client.account_exists(test_address).await {
        Ok(exists) => info!("Account {} exists: {}", test_address, exists),
        Err(e) => error!("Error checking account existence: {}", e),
    }

    match stellar_client.get_account(test_address).await {
        Ok(account) => {
            info!("Successfully fetched account details");
            info!("Account ID: {}", account.account_id);
            info!("Sequence: {}", account.sequence);
            info!("Number of balances: {}", account.balances.len());
            for balance in &account.balances {
                info!("Balance: {} {}", balance.balance, balance.asset_type);
            }
        }
        Err(e) => error!("Error fetching account: {}", e),
    }

    info!("Aframp backend service started successfully");
    
    Ok(())
}
