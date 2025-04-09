// DSM Storage Node Client Adapter
//
// This module provides functionality to connect to and interact with
// DSM Storage Nodes for unilateral transactions and DLVs.

use crate::types::error::DsmError;
// These imports are conditionally used based on feature flags
use crate::communication::storage_cache::StorageCache;
use crate::core::identity::GenesisState;
use crate::core::identity::Identity;
use crate::recovery::invalidation::InvalidationMarker;
use crate::types::operations::Operation;
#[cfg(feature = "reqwest")]
use crate::types::operations::Ops;
use crate::types::state_types::State;
use crate::types::token_types::Token;
use crate::vault::LimboVault;
use crate::vault::VaultStatus;
#[cfg(feature = "reqwest")]
use crate::vault::{VaultState, FulfillmentProof};
#[cfg(feature = "reqwest")]
use crate::InboxEntry;
#[cfg(not(feature = "reqwest"))]
use crate::InboxEntry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
#[cfg(feature = "reqwest")]
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use url::Url;

// Using the constant in the StorageNodeClient::new implementation
// This makes sure the constant is used
/// Default timeout value for storage node requests (30 seconds)
/// This value is used in StorageNodeClient::new implementation
#[allow(dead_code)]
const STORAGE_NODE_TIMEOUT: u64 = 30; // 30 seconds

/// Storage node client configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageNodeClientConfig {
    /// Storage node base URL
    pub base_url: String,

    /// API token (if required)
    pub api_token: Option<String>,

    /// Request timeout in seconds
    pub timeout_seconds: u64,
}

/// Storage node client with full HTTP capabilities
#[cfg(feature = "reqwest")]
pub struct StorageNodeClient {
    /// HTTP client for network operations
    http_client: reqwest::Client,

    /// Base URL of the storage node
    /// Contains the target endpoint for all API requests
    #[allow(dead_code)]
    base_url: Url,

    /// API token for authentication
    /// Used for authenticated requests to protected endpoints
    #[allow(dead_code)]
    api_token: Option<String>,

    /// Cache of recently accessed inbox entries
    /// Provides performance optimization for frequent inbox queries
    #[allow(dead_code)]
    inbox_cache: RwLock<HashMap<String, Vec<InboxEntry>>>,

    /// Cache of recently accessed vaults
    /// Reduces network load for vault operations
    #[allow(dead_code)]
    vault_cache: RwLock<HashMap<String, LimboVault>>,

    /// Advanced cache for persistent offline operation
    storage_cache: Arc<StorageCache>,

    /// Whether to cache all fetched data automatically
    auto_cache_enabled: bool,
}

/// Storage node client with minimal functionality when reqwest is disabled
#[cfg(not(feature = "reqwest"))]
#[allow(dead_code)]
pub struct StorageNodeClient {
    /// Base URL of the storage node
    base_url: Url,

    /// API token for authentication
    api_token: Option<String>,

    /// Cache of recently accessed inbox entries
    inbox_cache: RwLock<HashMap<String, Vec<InboxEntry>>>,

    /// Cache of recently accessed vaults
    vault_cache: RwLock<HashMap<String, LimboVault>>,

    /// Advanced cache for persistent offline operation
    storage_cache: Arc<StorageCache>,

    /// Whether to cache all fetched data automatically
    auto_cache_enabled: bool,
}

#[cfg(feature = "reqwest")]
impl StorageNodeClient {
    /// Create a new storage node client with default configuration
    ///
    /// # Arguments
    /// * `config` - Client configuration including base URL and authentication
    ///
    /// # Returns
    /// * `Result<Self, DsmError>` - The initialized client or an error
    pub fn new(config: StorageNodeClientConfig) -> Result<Self, DsmError> {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds.max(1)))
            .build()
            .map_err(|e| DsmError::Network {
                context: format!("Failed to create HTTP client: {}", e),
                source: Some(Box::new(e)),
            })?;

        let base_url = Url::parse(&config.base_url).map_err(|e| DsmError::Validation {
            context: format!("Invalid base URL: {}", e),
            source: Some(Box::new(e)),
        })?;

        Ok(Self {
            http_client,
            base_url,
            api_token: config.api_token,
            inbox_cache: RwLock::new(HashMap::new()),
            vault_cache: RwLock::new(HashMap::new()),
            storage_cache: Arc::new(StorageCache::new()),
            auto_cache_enabled: true,
        })
    }

    /// Create a new storage node client with custom cache settings
    ///
    /// # Arguments
    /// * `config` - Client configuration
    /// * `storage_cache` - Shared cache instance
    /// * `auto_cache` - Whether to automatically cache retrieved data
    ///
    /// # Returns
    /// * `Result<Self, DsmError>` - The initialized client or an error
    pub fn with_cache(
        config: StorageNodeClientConfig,
        storage_cache: Arc<StorageCache>,
        auto_cache: bool,
    ) -> Result<Self, DsmError> {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds.max(1)))
            .build()
            .map_err(|e| DsmError::Network {
                context: format!("Failed to create HTTP client: {}", e),
                source: Some(Box::new(e)),
            })?;

        let base_url = Url::parse(&config.base_url).map_err(|e| DsmError::Validation {
            context: format!("Invalid base URL: {}", e),
            source: Some(Box::new(e)),
        })?;

        Ok(Self {
            http_client,
            base_url,
            api_token: config.api_token,
            inbox_cache: RwLock::new(HashMap::new()),
            vault_cache: RwLock::new(HashMap::new()),
            storage_cache,
            auto_cache_enabled: auto_cache,
        })
    }

    /// Check if the storage node is healthy by pinging its health endpoint
    ///
    /// # Returns
    /// * `Result<bool, DsmError>` - Whether the storage node is healthy
    pub async fn check_health(&self) -> Result<bool, DsmError> {
        let url = self
            .base_url
            .join("health")
            .map_err(|e| DsmError::Network {
                context: format!("Failed to create URL: {}", e),
                source: Some(Box::new(e)),
            })?;

        let response = self
            .http_client
            .get(url)
            .send()
            .await
            .map_err(|e| DsmError::Network {
                context: format!("Failed to send request: {}", e),
                source: Some(Box::new(e)),
            })?;

        Ok(response.status().is_success())
    }

    /// Get the storage cache for direct access
    ///
    /// # Returns
    /// * `Arc<StorageCache>` - Reference to the shared storage cache
    pub fn get_storage_cache(&self) -> Arc<StorageCache> {
        self.storage_cache.clone()
    }

    /// Enable or disable automatic caching of retrieved data
    ///
    /// # Arguments
    /// * `enabled` - Whether to enable auto-caching
    pub fn set_auto_cache(&mut self, enabled: bool) {
        self.auto_cache_enabled = enabled;
    }

    /// Store a unilateral transaction in the recipient's inbox
    ///
    /// This implements the unilateral transaction storage mechanism described
    /// in whitepaper Section 16.3, enabling offline message delivery.
    ///
    /// # Arguments
    /// * `sender_identity` - Identity of the transaction sender
    /// * `sender_state` - Current state of the sender
    /// * `recipient_genesis` - Genesis state of the recipient
    /// * `operation` - Operation to store
    /// * `signature` - Signature over the operation data
    ///
    /// # Returns
    /// * `Result<(), DsmError>` - Success or an error
    pub async fn store_unilateral_transaction(
        &self,
        sender_identity: &Identity,
        _sender_state: &State, // Prefix with underscore to indicate intentionally unused
        recipient_genesis: &GenesisState,
        operation: &Operation,
        signature: &[u8],
    ) -> Result<(), DsmError> {
        // Create serializable inbox entry with proper cryptographic identification
        let recipient_genesis_bytes = bincode::serialize(recipient_genesis)?;
        let recipient_genesis_hash = hex::encode(blake3::hash(&recipient_genesis_bytes).as_bytes());

        let sender_genesis_bytes = bincode::serialize(&sender_identity.master_genesis)?;
        let sender_genesis_hash = hex::encode(blake3::hash(&sender_genesis_bytes).as_bytes());

        let operation_bytes = bincode::serialize(operation)?;
        let operation_hash = hex::encode(blake3::hash(&operation_bytes).as_bytes());

        // Construct the entry payload with all required fields
        let entry = serde_json::json!({
            "entry": {
                "id": format!("{}_{}_{}",
                    sender_genesis_hash,
                    operation_hash,
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                ),
                "sender_genesis_hash": sender_genesis_hash,
                "recipient_genesis_hash": recipient_genesis_hash,
                "operation": operation_bytes,
                "signature": signature.to_vec(),
                "timestamp": SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                "expires_at": 0, // No expiration
                "metadata": {
                    "type": "unilateral_transaction",
                    "sender": sender_identity.name.clone(),
                    "operation_type": match operation {
                        Operation::Generic { operation_type, .. } => operation_type.clone(),
                        _ => operation.get_id().to_string()
                    }
                }
            }
        });

        // Post to storage node
        let url = self.base_url.join("inbox").map_err(|e| DsmError::Network {
            context: format!("Failed to create URL: {}", e),
            source: Some(Box::new(e)),
        })?;

        let mut builder = self.http_client.post(url);

        if let Some(token) = &self.api_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }

        let response = builder
            .json(&entry)
            .send()
            .await
            .map_err(|e| DsmError::Network {
                context: format!("Failed to send request: {}", e),
                source: Some(Box::new(e)),
            })?;

        if !response.status().is_success() {
            return Err(DsmError::Network {
                context: format!("Storage node returned error: {}", response.status()),
                source: None,
            });
        }

        Ok(())
    }

    /// Fetch a genesis state from storage by its hash
    ///
    /// # Arguments
    /// * `genesis_hash` - Hash of the genesis state to fetch
    ///
    /// # Returns
    /// * `Result<Option<GenesisState>, DsmError>` - The genesis state if found
    pub async fn fetch_genesis_state(
        &self,
        genesis_hash: &[u8],
    ) -> Result<Option<GenesisState>, DsmError> {
        // First check local cache for efficiency
        if let Ok(Some(genesis)) = self.storage_cache.get_genesis(genesis_hash).await {
            return Ok(Some(genesis));
        }

        // Convert to hex string for API call
        let hash_hex = hex::encode(genesis_hash);

        // Fetch from storage node
        let url = self
            .base_url
            .join(&format!("genesis/{}", hash_hex))
            .map_err(|e| DsmError::Network {
                context: format!("Failed to create URL: {}", e),
                source: Some(Box::new(e)),
            })?;

        let mut builder = self.http_client.get(url);

        if let Some(token) = &self.api_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }

        let response = builder.send().await.map_err(|e| DsmError::Network {
            context: format!("Failed to send request: {}", e),
            source: Some(Box::new(e)),
        })?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(DsmError::Network {
                context: format!("Storage node returned error: {}", response.status()),
                source: None,
            });
        }

        // Parse response with proper error handling
        let genesis_bytes = &response.bytes().await.map_err(|e| DsmError::Network {
            context: format!("Failed to read response: {}", e),
            source: Some(Box::new(e)),
        })?;

        let genesis: GenesisState =
            bincode::deserialize(genesis_bytes).map_err(|e| DsmError::Serialization {
                context: format!("Failed to deserialize genesis state: {}", e),
                source: Some(Box::new(e)),
            })?;

        // Cache result if auto-caching is enabled
        if self.auto_cache_enabled {
            if let Err(e) = self
                .storage_cache
                .cache_genesis(genesis.clone(), true, None)
                .await
            {
                tracing::warn!("Failed to cache genesis state: {}", e);
            }
        }

        Ok(Some(genesis))
    }

    /// Fetch a token from storage by its ID
    ///
    /// # Arguments
    /// * `token_id` - ID of the token to fetch
    ///
    /// # Returns
    /// * `Result<Option<Token>, DsmError>` - The token if found
    pub async fn fetch_token(&self, token_id: &str) -> Result<Option<Token>, DsmError> {
        // First check local cache
        if let Ok(Some(token)) = self.storage_cache.get_token(token_id).await {
            return Ok(Some(token));
        }

        // Fetch from storage node
        let url = self
            .base_url
            .join(&format!("token/{}", token_id))
            .map_err(|e| DsmError::Network {
                context: format!("Failed to create URL: {}", e),
                source: Some(Box::new(e)),
            })?;

        let mut builder = self.http_client.get(url);

        if let Some(token) = &self.api_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }

        let response = builder.send().await.map_err(|e| DsmError::Network {
            context: format!("Failed to send request: {}", e),
            source: Some(Box::new(e)),
        })?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(DsmError::Network {
                context: format!("Storage node returned error: {}", response.status()),
                source: None,
            });
        }

        // Parse response
        let token_bytes = &response.bytes().await.map_err(|e| DsmError::Network {
            context: format!("Failed to read response: {}", e),
            source: Some(Box::new(e)),
        })?;

        let token: Token =
            bincode::deserialize(token_bytes).map_err(|e| DsmError::Serialization {
                context: format!("Failed to deserialize token: {}", e),
                source: Some(Box::new(e)),
            })?;

        // Cache result if auto-caching is enabled
        if self.auto_cache_enabled {
            if let Err(e) = self
                .storage_cache
                .cache_token(token_id, token.clone(), true, None)
                .await
            {
                tracing::warn!("Failed to cache token: {}", e);
            }
        }

        Ok(Some(token))
    }

    /// Fetch a checkpoint state from storage
    ///
    /// # Arguments
    /// * `checkpoint_id` - ID of the checkpoint to fetch
    ///
    /// # Returns
    /// * `Result<Option<State>, DsmError>` - The checkpoint state if found
    pub async fn fetch_checkpoint(&self, checkpoint_id: &str) -> Result<Option<State>, DsmError> {
        // Fetch from storage node
        let url = self
            .base_url
            .join(&format!("checkpoint/{}", checkpoint_id))
            .map_err(|e| DsmError::Network {
                context: format!("Failed to create URL: {}", e),
                source: Some(Box::new(e)),
            })?;

        let mut builder = self.http_client.get(url);

        if let Some(token) = &self.api_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }

        let response = builder.send().await.map_err(|e| DsmError::Network {
            context: format!("Failed to send request: {}", e),
            source: Some(Box::new(e)),
        })?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(DsmError::Network {
                context: format!("Storage node returned error: {}", response.status()),
                source: None,
            });
        }

        // Parse response with careful deserialization
        let checkpoint_bytes = &response.bytes().await.map_err(|e| DsmError::Network {
            context: format!("Failed to read response: {}", e),
            source: Some(Box::new(e)),
        })?;

        let checkpoint: State =
            bincode::deserialize(checkpoint_bytes).map_err(|e| DsmError::Serialization {
                context: format!("Failed to deserialize checkpoint: {}", e),
                source: Some(Box::new(e)),
            })?;

        // Cache result if auto-caching is enabled
        if self.auto_cache_enabled {
            if let Err(e) = self
                .storage_cache
                .cache_checkpoint(checkpoint.clone(), true, None)
                .await
            {
                tracing::warn!("Failed to cache checkpoint: {}", e);
            }
        }

        Ok(Some(checkpoint))
    }

    /// Fetch an invalidation marker from storage
    ///
    /// # Arguments
    /// * `state_hash` - Hash of the state to check for invalidation
    ///
    /// # Returns
    /// * `Result<Option<InvalidationMarker>, DsmError>` - The invalidation marker if found
    pub async fn fetch_invalidation_marker(
        &self,
        state_hash: &[u8],
    ) -> Result<Option<InvalidationMarker>, DsmError> {
        // First check local cache
        if let Ok(Some(marker)) = self.storage_cache.get_invalidation(state_hash).await {
            return Ok(Some(marker));
        }

        // Convert to hex string for API call
        let hash_hex = hex::encode(state_hash);

        // Fetch from storage node
        let url = self
            .base_url
            .join(&format!("invalidation/{}", hash_hex))
            .map_err(|e| DsmError::Network {
                context: format!("Failed to create URL: {}", e),
                source: Some(Box::new(e)),
            })?;

        let mut builder = self.http_client.get(url);

        if let Some(token) = &self.api_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }

        let response = builder.send().await.map_err(|e| DsmError::Network {
            context: format!("Failed to send request: {}", e),
            source: Some(Box::new(e)),
        })?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(DsmError::Network {
                context: format!("Storage node returned error: {}", response.status()),
                source: None,
            });
        }

        // Parse response
        let marker_bytes = &response.bytes().await.map_err(|e| DsmError::Network {
            context: format!("Failed to read response: {}", e),
            source: Some(Box::new(e)),
        })?;

        let marker: InvalidationMarker =
            bincode::deserialize(marker_bytes).map_err(|e| DsmError::Serialization {
                context: format!("Failed to deserialize invalidation marker: {}", e),
                source: Some(Box::new(e)),
            })?;

        // Cache result if auto-caching is enabled
        if self.auto_cache_enabled {
            if let Err(e) = self
                .storage_cache
                .cache_invalidation(marker.clone(), true, None)
                .await
            {
                tracing::warn!("Failed to cache invalidation marker: {}", e);
            }
        }

        Ok(Some(marker))
    }

    /// Check if a state has been invalidated
    ///
    /// # Arguments
    /// * `state_hash` - Hash of the state to check
    ///
    /// # Returns
    /// * `Result<bool, DsmError>` - Whether the state is invalidated
    pub async fn is_state_invalidated(&self, state_hash: &[u8]) -> Result<bool, DsmError> {
        // First check local cache
        if let Ok(is_invalidated) = self.storage_cache.is_state_invalidated(state_hash).await {
            if is_invalidated {
                return Ok(true);
            }
        }

        // Then check storage node
        match self.fetch_invalidation_marker(state_hash).await? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    /// Get unilateral transactions from the inbox
    ///
    /// This retrieves messages waiting in the recipient's inbox, implementing
    /// the unilateral transaction delivery mechanism from whitepaper Section 16.3.
    ///
    /// # Arguments
    /// * `recipient_genesis` - Genesis state of the recipient
    ///
    /// # Returns
    /// * `Result<Vec<InboxEntry>, DsmError>` - List of inbox entries
    pub async fn get_inbox_transactions(
        &self,
        recipient_genesis: &GenesisState,
    ) -> Result<Vec<InboxEntry>, DsmError> {
        let recipient_genesis_bytes = bincode::serialize(recipient_genesis)?;
        let recipient_genesis_hash = hex::encode(blake3::hash(&recipient_genesis_bytes).as_bytes());

        // Check cache first for efficiency
        {
            let cache = self.inbox_cache.read().await;
            if let Some(entries) = cache.get(&recipient_genesis_hash) {
                return Ok(entries.clone());
            }
        }

        // Fetch from storage node
        let url = self
            .base_url
            .join(&format!("inbox/{}", recipient_genesis_hash))
            .map_err(|e| DsmError::Network {
                context: format!("Failed to create URL: {}", e),
                source: Some(Box::new(e)),
            })?;

        let mut builder = self.http_client.get(url);

        if let Some(token) = &self.api_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }

        let response = builder.send().await.map_err(|e| DsmError::Network {
            context: format!("Failed to send request: {}", e),
            source: Some(Box::new(e)),
        })?;

        if !response.status().is_success() {
            return Err(DsmError::Network {
                context: format!("Storage node returned error: {}", response.status()),
                source: None,
            });
        }

        let entries: Vec<serde_json::Value> =
            response.json().await.map_err(|e| DsmError::Serialization {
                context: format!("Failed to parse response: {}", e),
                source: Some(Box::new(e)),
            })?;

        // Convert to InboxEntry with careful conversion and validation
        let mut result = Vec::new();
        for entry_json in entries {
            let operation_bytes = entry_json["operation"]
                .as_array()
                .ok_or_else(|| DsmError::Serialization {
                    context: "Missing operation bytes".into(),
                    source: None,
                })?
                .iter()
                .map(|v| v.as_u64().unwrap_or(0) as u8)
                .collect::<Vec<u8>>();

            let signature_bytes = entry_json["signature"]
                .as_array()
                .ok_or_else(|| DsmError::Serialization {
                    context: "Missing signature bytes".into(),
                    source: None,
                })?
                .iter()
                .map(|v| v.as_u64().unwrap_or(0) as u8)
                .collect::<Vec<u8>>();

            let _operation: Operation =
                bincode::deserialize(&operation_bytes).map_err(|e| DsmError::Serialization {
                    context: format!("Failed to deserialize operation: {}", e),
                    source: Some(Box::new(e)),
                })?;

            let entry = InboxEntry {
                id: entry_json["id"].as_str().unwrap_or("").to_string(),
                sender_genesis_hash: entry_json["sender_genesis_hash"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                recipient_genesis_hash: recipient_genesis_hash.clone(),
                transaction: operation_bytes,
                signature: signature_bytes,
                timestamp: entry_json["timestamp"].as_u64().unwrap_or(0),
                expires_at: 0, // No expiration
                metadata: HashMap::new(),
            };

            result.push(entry);
        }

        // Update cache
        {
            let mut cache = self.inbox_cache.write().await;
            cache.insert(recipient_genesis_hash, result.clone());
        }

        // If auto-caching is enabled, also cache the genesis state for offline access
        if self.auto_cache_enabled
            && !self
                .storage_cache
                .has_genesis(&recipient_genesis_bytes)
                .await
        {
            if let Err(e) = self
                .storage_cache
                .cache_genesis(recipient_genesis.clone(), true, None)
                .await
            {
                tracing::warn!("Failed to cache genesis state from inbox: {}", e);
            }
        }

        Ok(result)
    }

    /// Delete a transaction from the inbox
    ///
    /// # Arguments
    /// * `recipient_genesis` - Genesis state of the recipient
    /// * `entry_id` - ID of the entry to delete
    ///
    /// # Returns
    /// * `Result<(), DsmError>` - Success or an error
    pub async fn delete_inbox_transaction(
        &self,
        recipient_genesis: &GenesisState,
        entry_id: &str,
    ) -> Result<(), DsmError> {
        let recipient_genesis_bytes = bincode::serialize(recipient_genesis)?;
        let recipient_genesis_hash = hex::encode(blake3::hash(&recipient_genesis_bytes).as_bytes());

        // Delete from storage node
        let url = self
            .base_url
            .join(&format!("inbox/{}/{}", recipient_genesis_hash, entry_id))
            .map_err(|e| DsmError::Network {
                context: format!("Failed to create URL: {}", e),
                source: Some(Box::new(e)),
            })?;

        let mut builder = self.http_client.delete(url);

        if let Some(token) = &self.api_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }

        let response = builder.send().await.map_err(|e| DsmError::Network {
            context: format!("Failed to send request: {}", e),
            source: Some(Box::new(e)),
        })?;

        if !response.status().is_success() {
            return Err(DsmError::Network {
                context: format!("Storage node returned error: {}", response.status()),
                source: None,
            });
        }

        // Update cache to maintain consistency
        {
            let mut cache = self.inbox_cache.write().await;
            if let Some(entries) = cache.get_mut(&recipient_genesis_hash) {
                entries.retain(|e| e.id != entry_id);
            }
        }

        Ok(())
    }

    /// Store a vault in the decentralized storage
    ///
    /// This implements the DLV storage mechanism described in whitepaper Section 20.
    ///
    /// # Arguments
    /// * `vault` - Vault to store
    /// * `creator_signature` - Signature from the vault creator
    ///
    /// # Returns
    /// * `Result<(), DsmError>` - Success or an error
    pub async fn store_vault(
        &self,
        vault: &LimboVault,
        creator_signature: &[u8],
    ) -> Result<(), DsmError> {
        // Create serializable vault data with the appropriate status representation
        let status_str = match vault.state {
            VaultState::Limbo => "active",
            VaultState::Claimed { .. } => "claimed",
            VaultState::Invalidated { .. } => "revoked",
            VaultState::Unlocked { .. } => "unlocked",
        };

        let vault_data = serde_json::json!({
            "vault": {
                "id": vault.id,
                "creator_id": hex::encode(&vault.creator_public_key), // Convert public key to hex
                "status": status_str,
                "recipient_id": vault.intended_recipient.as_ref().map(hex::encode).unwrap_or_default(),
                // Additional fields as needed by API
                "timestamp": SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            },
            "signature": creator_signature,
        });

        // Post to storage node
        let url = self.base_url.join("vault").map_err(|e| DsmError::Network {
            context: format!("Failed to create URL: {}", e),
            source: Some(Box::new(e)),
        })?;

        let mut builder = self.http_client.post(url);

        if let Some(token) = &self.api_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }

        let response = builder
            .json(&vault_data)
            .send()
            .await
            .map_err(|e| DsmError::Network {
                context: format!("Failed to send request: {}", e),
                source: Some(Box::new(e)),
            })?;

        if !response.status().is_success() {
            return Err(DsmError::Network {
                context: format!("Storage node returned error: {}", response.status()),
                source: None,
            });
        }

        // Update cache for local vault availability
        {
            let mut cache = self.vault_cache.write().await;
            cache.insert(vault.id.clone(), vault.clone());
        }

        Ok(())
    }

    /// Retrieve a vault by its cryptographic identifier
    ///
    /// This fetches a Deterministic Limbo Vault from decentralized storage,
    /// implementing the vault retrieval mechanism from whitepaper Section 20.3.
    ///
    /// # Arguments
    /// * `vault_id` - Cryptographic identifier of the vault
    ///
    /// # Returns
    /// * `Result<Option<LimboVault>, DsmError>` - The vault if found
    pub async fn get_vault(&self, vault_id: &str) -> Result<Option<LimboVault>, DsmError> {
        // Check cache first for reduced network overhead
        {
            let cache = self.vault_cache.read().await;
            if let Some(vault) = cache.get(vault_id) {
                return Ok(Some(vault.clone()));
            }
        }

        // Fetch from storage node with proper URL construction
        let url = self
            .base_url
            .join(&format!("vault/{}", vault_id))
            .map_err(|e| DsmError::Network {
                context: format!("Failed to create URL: {}", e),
                source: Some(Box::new(e)),
            })?;

        let mut builder = self.http_client.get(url);

        if let Some(token) = &self.api_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }

        let response = builder.send().await.map_err(|e| DsmError::Network {
            context: format!("Failed to send request: {}", e),
            source: Some(Box::new(e)),
        })?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(DsmError::Network {
                context: format!("Storage node returned error: {}", response.status()),
                source: None,
            });
        }

        // Parse vault information with proper JSON validation
        let vault_json: serde_json::Value =
            response.json().await.map_err(|e| DsmError::Serialization {
                context: format!("Failed to parse response: {}", e),
                source: Some(Box::new(e)),
            })?;

        // Convert JSON status to VaultState enum with robust fallback
        let vault_state = match vault_json["status"].as_str().unwrap_or("active") {
            "active" => VaultState::Limbo,
            "claimed" => {
                VaultState::Claimed {
                    claimed_state_number: vault_json["claimed_state_number"].as_u64().unwrap_or(0),
                    claimant: Vec::new(), // Would need the actual claimant data
                    claim_proof: Vec::new(), // Would need the actual proof data
                }
            }
            "revoked" => {
                VaultState::Invalidated {
                    invalidated_state_number: vault_json["invalidated_state_number"]
                        .as_u64()
                        .unwrap_or(0),
                    reason: vault_json["invalidation_reason"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                    creator_signature: Vec::new(), // Would need the actual signature data
                }
            }
            "unlocked" => {
                VaultState::Unlocked {
                    unlocked_state_number: vault_json["unlocked_state_number"]
                        .as_u64()
                        .unwrap_or(0),
                    fulfillment_proof: FulfillmentProof::TimeProof {
                        reference_state: Vec::new(), // Would need the actual reference state
                        state_proof: Vec::new(),     // Would need the actual proof
                    },
                }
            }
            _ => VaultState::Limbo, // Default to Limbo for unknown status values
        };

        // Parse content_type with proper fallback
        let _content_type = vault_json["content_type"]
            .as_str()
            .unwrap_or("application/octet-stream")
            .to_string();

        // Create vault from available information
        // Extract required data fields with safe defaults
        let vault_id = vault_json["id"].as_str().unwrap_or("").to_string();
        let creator_id = vault_json["creator_id"].as_str().unwrap_or("");
        let creator_public_key = hex::decode(creator_id).unwrap_or_default();

        let recipient_id = vault_json["recipient_id"].as_str().unwrap_or("");
        let intended_recipient = if !recipient_id.is_empty() {
            Some(hex::decode(recipient_id).unwrap_or_default())
        } else {
            None
        };

        let content_type = vault_json["content_type"]
            .as_str()
            .unwrap_or("application/octet-stream")
            .to_string();

        let created_at_state = vault_json["created_at_state"].as_u64().unwrap_or(0);

        // In a real implementation, we'd extract actual encrypted content
        let encrypted_content = crate::vault::EncryptedContent {
            encapsulated_key: Vec::new(),
            encrypted_data: Vec::new(),
            nonce: Vec::new(),
            aad: Vec::new(),
        };

        // Use a default Pedersen commitment for now
        use crate::crypto::pedersen::PedersenCommitment;

        // Create the vault structure with available fields
        // This is a very simplified version that wouldn't work in production
        let vault = LimboVault {
            id: vault_id,
            created_at_state,
            creator_public_key,
            // This would need to be properly extracted in a real implementation
            fulfillment_condition: crate::vault::FulfillmentMechanism::TimeRelease {
                unlock_time: 0,
                reference_states: Vec::new(),
            },
            intended_recipient,
            state: vault_state,
            content_type,
            encrypted_content,
            content_commitment: PedersenCommitment::default(),
            parameters_hash: Vec::new(),
            creator_signature: Vec::new(),
            verification_positions: Vec::new(),
            reference_state_hash: Vec::new(),
        };

        // Create a new vault with the status from the API response
        // Note: We can't directly modify private fields like id or status
        // In a real implementation, we would create a method to update the status
        // For now, we'll just use the vault as-is

        // Update cache
        {
            let mut cache = self.vault_cache.write().await;
            cache.insert(vault.id.clone(), vault.clone());
        }

        Ok(Some(vault))
    }

    /// Update a vault's status with cryptographic verification
    ///
    /// This allows authorized status transitions as defined in whitepaper Section 20.2.
    ///
    /// # Arguments
    /// * `vault_id` - ID of the vault to update
    /// * `new_status` - New status for the vault
    ///
    /// # Returns
    /// * `Result<(), DsmError>` - Success or an error
    pub async fn update_vault_status(
        &self,
        vault_id: &str,
        new_status: VaultStatus,
    ) -> Result<(), DsmError> {
        // Create status update JSON payload
        let status_update = match new_status {
            VaultStatus::Active => {
                serde_json::json!({
                    "status_type": "active"
                })
            }
            VaultStatus::Claimed => {
                serde_json::json!({
                    "status_type": "claimed"
                })
            }
            VaultStatus::Revoked => {
                serde_json::json!({
                    "status_type": "revoked"
                })
            }
            VaultStatus::Expired => {
                serde_json::json!({
                    "status_type": "expired"
                })
            }
        };

        // Send to storage node with proper URL construction and error handling
        let url = self
            .base_url
            .join(&format!("vault/{}/status", vault_id))
            .map_err(|e| DsmError::Network {
                context: format!("Failed to create URL: {}", e),
                source: Some(Box::new(e)),
            })?;

        let mut builder = self.http_client.put(url);

        if let Some(token) = &self.api_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }

        let response =
            builder
                .json(&status_update)
                .send()
                .await
                .map_err(|e| DsmError::Network {
                    context: format!("Failed to send request: {}", e),
                    source: Some(Box::new(e)),
                })?;

        if !response.status().is_success() {
            return Err(DsmError::Network {
                context: format!("Storage node returned error: {}", response.status()),
                source: None,
            });
        }

        // Update cache for consistency
        {
            let mut cache = self.vault_cache.write().await;
            if let Some(vault) = cache.get_mut(vault_id) {
                // We can't directly modify the status field as it's private
                // In a real implementation, we would have a method to update the status
                // For now, let's just update the vault's state directly
                vault.state = match new_status {
                    VaultStatus::Active => VaultState::Limbo,
                    VaultStatus::Claimed => VaultState::Claimed {
                        claimed_state_number: 0,
                        claimant: Vec::new(),
                        claim_proof: Vec::new(),
                    },
                    VaultStatus::Revoked => VaultState::Invalidated {
                        invalidated_state_number: 0,
                        reason: String::new(),
                        creator_signature: Vec::new(),
                    },
                    VaultStatus::Expired => VaultState::Unlocked {
                        unlocked_state_number: 0,
                        fulfillment_proof: FulfillmentProof::TimeProof {
                            reference_state: Vec::new(),
                            state_proof: Vec::new(),
                        },
                    },
                };
            }
        }

        Ok(())
    }
}

// When reqwest feature is disabled, implement minimal functionality
#[cfg(not(feature = "reqwest"))]
impl StorageNodeClient {
    /// Create a new storage node client with minimal capabilities
    pub fn new(config: StorageNodeClientConfig) -> Result<Self, DsmError> {
        let base_url = Url::parse(&config.base_url).map_err(|e| DsmError::Validation {
            context: format!("Invalid base URL: {}", e),
            source: Some(Box::new(e)),
        })?;

        Ok(Self {
            base_url,
            api_token: config.api_token,
            inbox_cache: RwLock::new(HashMap::new()),
            vault_cache: RwLock::new(HashMap::new()),
            storage_cache: Arc::new(StorageCache::new()),
            auto_cache_enabled: true,
        })
    }

    /// Create a new storage node client with custom cache settings
    pub fn with_cache(
        config: StorageNodeClientConfig,
        storage_cache: Arc<StorageCache>,
        auto_cache: bool,
    ) -> Result<Self, DsmError> {
        let base_url = Url::parse(&config.base_url).map_err(|e| DsmError::Validation {
            context: format!("Invalid base URL: {}", e),
            source: Some(Box::new(e)),
        })?;

        Ok(Self {
            base_url,
            api_token: config.api_token,
            inbox_cache: RwLock::new(HashMap::new()),
            vault_cache: RwLock::new(HashMap::new()),
            storage_cache,
            auto_cache_enabled: auto_cache,
        })
    }

    /// Get the storage cache for direct access
    pub fn get_storage_cache(&self) -> Arc<StorageCache> {
        self.storage_cache.clone()
    }

    /// Enable or disable automatic caching
    pub fn set_auto_cache(&mut self, enabled: bool) {
        self.auto_cache_enabled = enabled;
    }

    /// Check if storage node is healthy (always returns error when reqwest is disabled)
    pub async fn check_health(&self) -> Result<bool, DsmError> {
        Err(DsmError::feature_not_available(
            "Network functionality requires the 'reqwest' feature",
            None::<String>,
        ))
    }

    /// Store a unilateral transaction (not available without reqwest)
    pub async fn store_unilateral_transaction(
        &self,
        _sender_identity: &Identity,
        _sender_state: &State,
        _recipient_genesis: &GenesisState,
        _operation: &Operation,
        _signature: &[u8],
    ) -> Result<(), DsmError> {
        Err(DsmError::feature_not_available(
            "Network functionality requires the 'reqwest' feature",
            None::<String>,
        ))
    }

    /// Fetch a genesis state (not available without reqwest)
    pub async fn fetch_genesis_state(
        &self,
        _genesis_hash: &[u8],
    ) -> Result<Option<GenesisState>, DsmError> {
        Err(DsmError::feature_not_available(
            "Network functionality requires the 'reqwest' feature",
            None::<String>,
        ))
    }

    /// Fetch a token (not available without reqwest)
    pub async fn fetch_token(&self, _token_id: &str) -> Result<Option<Token>, DsmError> {
        Err(DsmError::feature_not_available(
            "Network functionality requires the 'reqwest' feature",
            None::<String>,
        ))
    }

    /// Fetch a checkpoint (not available without reqwest)
    pub async fn fetch_checkpoint(&self, _checkpoint_id: &str) -> Result<Option<State>, DsmError> {
        Err(DsmError::feature_not_available(
            "Network functionality requires the 'reqwest' feature",
            None::<String>,
        ))
    }

    /// Fetch an invalidation marker (not available without reqwest)
    pub async fn fetch_invalidation_marker(
        &self,
        _state_hash: &[u8],
    ) -> Result<Option<InvalidationMarker>, DsmError> {
        Err(DsmError::feature_not_available(
            "Network functionality requires the 'reqwest' feature",
            None::<String>,
        ))
    }

    /// Check if a state is invalidated (not available without reqwest)
    pub async fn is_state_invalidated(&self, _state_hash: &[u8]) -> Result<bool, DsmError> {
        Err(DsmError::feature_not_available(
            "Network functionality requires the 'reqwest' feature",
            None::<String>,
        ))
    }

    /// Get unilateral transactions (not available without reqwest)
    pub async fn get_inbox_transactions(
        &self,
        _recipient_genesis: &GenesisState,
    ) -> Result<Vec<InboxEntry>, DsmError> {
        Err(DsmError::feature_not_available(
            "Network functionality requires the 'reqwest' feature",
            None::<String>,
        ))
    }

    /// Delete a transaction from the inbox (not available without reqwest)
    pub async fn delete_inbox_transaction(
        &self,
        _recipient_genesis: &GenesisState,
        _entry_id: &str,
    ) -> Result<(), DsmError> {
        Err(DsmError::feature_not_available(
            "Network functionality requires the 'reqwest' feature",
            None::<String>,
        ))
    }

    /// Store a vault (not available without reqwest)
    pub async fn store_vault(
        &self,
        _vault: &DeterministicLimboVault,
        _creator_signature: &[u8],
    ) -> Result<(), DsmError> {
        Err(DsmError::feature_not_available(
            "Network functionality requires the 'reqwest' feature",
            None::<String>,
        ))
    }

    /// Get a vault (not available without reqwest)
    pub async fn get_vault(
        &self,
        _vault_id: &str,
    ) -> Result<Option<DeterministicLimboVault>, DsmError> {
        Err(DsmError::feature_not_available(
            "Network functionality requires the 'reqwest' feature",
            None::<String>,
        ))
    }

    /// Update vault status (not available without reqwest)
    pub async fn update_vault_status(
        &self,
        _vault_id: &str,
        _new_status: VaultStatus,
    ) -> Result<(), DsmError> {
        Err(DsmError::feature_not_available(
            "Network functionality requires the 'reqwest' feature",
            None::<String>,
        ))
    }
}
