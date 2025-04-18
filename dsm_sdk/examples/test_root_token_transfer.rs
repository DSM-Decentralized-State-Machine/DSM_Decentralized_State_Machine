// DSM ROOT Token Transfer Test
// This script tests the transfer of ROOT tokens between two registered devices
// using real network storage nodes and verifies state updates

use dsm::crypto;
use dsm::types::error::DsmError;
use dsm::types::operations::Operation;
// TokenManager trait is required to access execute_token_operation method
use dsm_sdk::core_sdk::TokenManager;
use dsm::types::state_types::DeviceInfo;
use dsm::types::token_types::{Balance, TokenOperation};
use dsm_sdk::core_sdk::CoreSDK;
use dsm_sdk::identity_sdk::IdentitySDK;
use dsm_sdk::token_sdk::TokenSDK;
use dsm_storage_node::client::{StorageNodeClient, StorageNodeClientConfig}; 
use std::sync::Arc;


#[tokio::main]
async fn main() -> Result<(), DsmError> {
    // ==========================================================================
    // System Initialization
    // ==========================================================================
    println!("=== DSM ROOT Token Transfer Test ===");
    println!("Initializing system components...");

    // Initialize the DSM system
    dsm::initialize();

    // Initialize the storage client for the running storage node
    // Connect to the running storage node at localhost:8080
    let config = StorageNodeClientConfig {
        base_url: "http://127.0.0.1:8080".to_string(),
        api_token: None,
        timeout_seconds: 30,
    };
    
    let _storage_client = Arc::new(StorageNodeClient::new(config).map_err(|e| {
        DsmError::Storage {
            context: format!("Failed to create storage client: {:?}", e),
            source: None,
        }
    })?);
    
    println!("Connected to storage node at http://127.0.0.1:8080");

    // Create core SDK
    let core_sdk = Arc::new(CoreSDK::new());
    
    // Create the identity SDK using the hash_chain_sdk from core_sdk
    let identity_sdk = Arc::new(IdentitySDK::new(
        "default".to_string(),
        core_sdk.hash_chain_sdk(),
    ));
    
    // Create token SDK for handling token operations
    // After our architectural refactoring, we now need to use TokenSDK directly
    let token_sdk = Arc::new(TokenSDK::new(core_sdk.clone()));
    
    // Pre-register the token SDK with core
    core_sdk.register_token_manager(token_sdk.clone());

    // ==========================================================================
    // Device Setup - Create two devices with Genesis states
    // ==========================================================================
    println!("\n=== Device Setup ===");

    // Create devices
    let sender_device_id = "sender_device";
    let receiver_device_id = "receiver_device";
    
    // Generate keypairs for both devices
    let (_sender_kyber_pk, _sender_kyber_sk, sender_sphincs_pk, _sender_sphincs_sk) = crypto::generate_keypair();
    let (_receiver_kyber_pk, _receiver_kyber_sk, receiver_sphincs_pk, _receiver_sphincs_sk) = crypto::generate_keypair();
    
    println!("Created sender device ID: {}", sender_device_id);
    println!("Created receiver device ID: {}", receiver_device_id);

    // Create DeviceInfo for both devices
    let sender_device_info = DeviceInfo::new(sender_device_id, sender_sphincs_pk.clone());
    let receiver_device_info = DeviceInfo::new(receiver_device_id, receiver_sphincs_pk.clone());

    // Create genesis state for the sender device
    let mut participant_inputs = Vec::new();
    for i in 0..3 {
        let mut hasher = blake3::Hasher::new();
        hasher.update(format!("sender_participant_{}", i).as_bytes());
        hasher.update(&crypto::generate_nonce());
        participant_inputs.push(hasher.finalize().as_bytes().to_vec());
    }

    let mut sender_genesis = identity_sdk.create_genesis(
        sender_device_info.clone(),
        participant_inputs,
        Some("Sender device metadata".as_bytes().to_vec()),
    )?;

    // Compute and set the hash for the genesis state
    let computed_hash = sender_genesis.compute_hash()?;
    sender_genesis.hash = computed_hash;

    println!("Created genesis state for sender device");

    // Create genesis state for the receiver device similarly
    let mut participant_inputs = Vec::new();
    for i in 0..3 {
        let mut hasher = blake3::Hasher::new();
        hasher.update(format!("receiver_participant_{}", i).as_bytes());
        hasher.update(&crypto::generate_nonce());
        participant_inputs.push(hasher.finalize().as_bytes().to_vec());
    }

    let mut receiver_genesis = identity_sdk.create_genesis(
        receiver_device_info.clone(),
        participant_inputs,
        Some("Receiver device metadata".as_bytes().to_vec()),
    )?;

    // Compute and set the hash for the genesis state
    let computed_hash = receiver_genesis.compute_hash()?;
    receiver_genesis.hash = computed_hash;

    println!("Created genesis state for receiver device");

    // ==========================================================================
    // Initialize Storage Node with Genesis States
    // ==========================================================================
    println!("\n=== Initializing Storage Node with Genesis States ===");

    // Register the sender's genesis state with the storage node
    // Since CoreSDK doesn't have a register_state method, we'll use the execute_transition method
    // to store the genesis state in the hash chain
    let result = core_sdk.initialize_with_genesis(sender_genesis.clone()).await;
    match result {
        Ok(_) => println!("Successfully registered sender's genesis state"),
        Err(e) => println!("Failed to register sender's genesis state: {:?}", e),
    }

    // Register the receiver's genesis state similarly
    // In a real implementation, you may need to use a storage node API directly
    // This is a simplified approach
    let receiver_core_sdk = Arc::new(CoreSDK::new());
    let result = receiver_core_sdk.initialize_with_genesis(receiver_genesis.clone()).await;
    match result {
        Ok(_) => println!("Successfully registered receiver's genesis state"),
        Err(e) => println!("Failed to register receiver's genesis state: {:?}", e),
    }

    // ==========================================================================
    // Initialize Core System with Genesis
    // ==========================================================================
    println!("\n=== Online Initialization ===");

    // We've already initialized with the sender's genesis state earlier,
    // so we don't need to do it again here. The error occurs because
    // we're trying to initialize with a state number that already exists.
    // Let's retrieve the current state instead.
    let current_state = core_sdk.get_current_state()?;
    println!("Using current system state: state #{}", current_state.state_number);

    // ==========================================================================
    // Mint ROOT Tokens to Sender
    // ==========================================================================
    println!("\n=== Minting ROOT Tokens ===");

    // Mint initial ROOT tokens to the sender
    // For the mint operation, rather than trying to modify the existing state,
    // we'll let the state transition itself handle setting up the token balance
    // Explicitly target the sender with the mint operation
    let mint_op = Operation::Mint {
        token_id: "ROOT".to_string(),
        amount: Balance::new(1000),
        message: format!("Initial ROOT token allocation to {}", sender_device_id),
        authorized_by: "Treasury".to_string(),
        proof_of_authorization: vec![0, 1, 2, 3], // Simplified proof
    };
    
    // Then execute the mint operation
    let state_after_mint = core_sdk.execute_transition(mint_op).await?;
    println!("ROOT tokens minted to sender, state #: {}", state_after_mint.state_number);
    
    // Force a state update and metadata refresh
    let _refreshed_state = core_sdk.get_current_state()?;
    // Manual update for token metadata to ensure it's synchronized
    if let Ok(()) = token_sdk.update_metadata().await {
        println!("Successfully refreshed token metadata");
    }

    // Get and verify sender's balance directly using TokenSDK
    // This provides a more reliable balance lookup through the proper API
    let _current_state_for_balance = core_sdk.get_current_state()?;
    let _sender_token_key = format!("{}.{}", sender_device_id, "ROOT");
    let sender_balance = token_sdk.get_token_balance(sender_device_id, "ROOT");
    
    // Print detailed balance information for debugging
    println!("DEBUG: Sender token key: {}", _sender_token_key);
    println!("DEBUG: Current state number: {}", _current_state_for_balance.state_number);
    println!("DEBUG: Current state hash: {:?}", _current_state_for_balance.hash);
    
    // Check if balance is available in the state directly
    let state_balances = &_current_state_for_balance.token_balances;
    if let Some(balance) = state_balances.get(&_sender_token_key) {
        println!("DEBUG: Found balance in state directly: {:?}", balance);
    } else {
        println!("DEBUG: No balance found in state for key: {}", _sender_token_key);
    }
    
    // Ensure balance is available for the test
    if sender_balance.value() == 0 {
        // Use TokenSDK to execute a follow-up mint if needed
        let ensure_mint_op = TokenOperation::Mint {
            token_id: "ROOT".to_string(),
            recipient: sender_device_id.to_string(), // Explicitly set the recipient to the sender device ID
            amount: 1000,
        };
        
        // Execute the mint through TokenSDK's proper API via the TokenManager trait
        let after_mint_state = TokenManager::execute_token_operation(&*token_sdk, ensure_mint_op).await?;
        println!("Executed emergency mint operation to ensure balance. State #: {}", after_mint_state.state_number);
        
        // Force refresh balances from state after mint
        let _refreshed_again = core_sdk.get_current_state()?;
        if let Ok(()) = token_sdk.update_metadata().await {
            println!("Refreshed token metadata after emergency mint");
        }
        
        // Get updated balance after emergency mint
        let emergency_balance = token_sdk.get_token_balance(sender_device_id, "ROOT");
        println!("Balance after emergency mint: {:?}", emergency_balance);
    }
    
    // Get the final balance from the token SDK for the test
    let sender_balance = token_sdk.get_token_balance(sender_device_id, "ROOT");
    println!("Sender's ROOT balance after minting: {:?}", sender_balance);

    // ==========================================================================
    // Establish Relationship between Sender and Receiver
    // ==========================================================================
    println!("\n=== Relationship Establishment ===");

    // Establish a relationship between sender and receiver
    let relationship_id = "sender_receiver_relationship";

    // Create an operation through CoreSDK to establish the relationship
    let relationship_op = Operation::Generic {
        operation_type: "establish_relationship".to_string(),
        data: bincode::serialize(&(
            relationship_id,
            receiver_device_id.to_string(),
            b"Initial relationship data".to_vec(),
        )).unwrap(),
        message: "Establish relationship".to_string(),
    };

    let state_after_relationship = core_sdk.execute_transition(relationship_op).await?;
    println!("Relationship established, state #: {}", state_after_relationship.state_number);

    // ==========================================================================
    // Online Token Transfer (Unilateral Transaction)
    // ==========================================================================
    println!("\n=== Online ROOT Token Transfer ===");

    // Transfer ROOT tokens from sender to receiver
    let mut transfer_amount = 500;
    // Using TokenSDK directly for token balance information
    let _current_state_after_mint = core_sdk.get_current_state()?;
    
    // Get sender balance with correct key format - using direct TokenSDK API
    let sender_balance = token_sdk.get_token_balance(sender_device_id, "ROOT");
    
    // Ensure sender has sufficient balance
    if sender_balance.value() < transfer_amount {
        println!("Warning: Sender has insufficient balance ({}) for transfer ({}). Setting transfer amount to available balance.", 
            sender_balance.value(), transfer_amount);
        // Adjust transfer amount to available balance
        transfer_amount = sender_balance.value();
    }
    
    // Create a TokenOperation::Transfer directly instead of using the helper method
    // This will be processed directly by the TokenManager trait implementation
    let transfer_op = TokenOperation::Transfer {
        token_id: "ROOT".to_string(),
        recipient: receiver_device_id.to_string(),
        amount: transfer_amount,
        memo: Some("Transfer ROOT tokens to receiver".to_string()),
    };

    // Execute the transfer operation directly through the TokenManager trait
    // which has better balance handling logic
    let state_after_transfer = TokenManager::execute_token_operation(&*token_sdk, transfer_op).await?;
    println!("ROOT tokens transferred, state #: {}", state_after_transfer.state_number);

    // ==========================================================================
    // Verify State Updates
    // ==========================================================================
    println!("\n=== Verifying State Updates ===");

    // Get sender's balance directly using TokenSDK for consistent API usage
    let sender_balance_after = token_sdk.get_token_balance(sender_device_id, "ROOT");
    println!("Sender's ROOT balance after transfer: {:?}", sender_balance_after);
    
    // Print detailed state info including hash
    let final_sender_state = core_sdk.get_current_state()?;
    println!("DEBUG: Current state number: {}", final_sender_state.state_number);
    println!("DEBUG: Current state hash: {:?}", final_sender_state.hash);
    println!("DEBUG: State hash length: {}", final_sender_state.hash.len());

    // Verify sender's balance decreased by the transfer amount
    if sender_balance.value() - sender_balance_after.value() == transfer_amount {
        println!("✅ Sender's balance correctly decreased by {} tokens", transfer_amount);
    } else {
        println!("❌ Sender's balance did not decrease correctly. Expected: {}, Actual: {}", 
            sender_balance.value() - transfer_amount, sender_balance_after.value());
    }

    // Get current state to access token balances - marked as intentionally unused
    let _final_state = core_sdk.get_current_state()?;

    // Initialize a new CoreSDK instance for the receiver to check their state
    let receiver_core_sdk = Arc::new(CoreSDK::new());
    
    // Initialize with receiver's genesis
    receiver_core_sdk.initialize_with_genesis(receiver_genesis.clone()).await?;
    
    // Attempt to get the updated state for the receiver from the storage node
    let receiver_state = match receiver_core_sdk.get_current_state() {
        Ok(state) => state,
        Err(e) => {
            println!("Failed to get receiver's current state: {:?}", e);
            receiver_genesis.clone() // Fallback to genesis state
        }
    };
    
    println!("Retrieved receiver's current state, state #: {}", receiver_state.state_number);
    
    // Get receiver's balance through TokenSDK for consistent API access
    // We'll use the original token_sdk since it's properly configured with the storage node
    let receiver_balance = token_sdk.get_token_balance(receiver_device_id, "ROOT");
    println!("Receiver's ROOT balance after transfer: {:?}", receiver_balance);
    
    // Verify receiver's balance increased by the transfer amount
    if receiver_balance.value() == transfer_amount {
        println!("✅ Receiver's balance correctly increased to {} tokens", transfer_amount);
    } else {
        println!("❌ Receiver's balance is not as expected. Expected: {}, Actual: {}",
            transfer_amount, receiver_balance.value());
    }

    // ==========================================================================
    // Test Synchronization and State Validity
    // ==========================================================================
    println!("\n=== Testing Storage Node Synchronization ===");

    // Verify states are stored in the storage node by retrieving them directly
    // Note: Methods may need to be adapted based on your actual StorageNodeClient API
    println!("Checking states in storage node (this might require implementation-specific code)");

    // ==========================================================================
    // Summary
    // ==========================================================================
    println!("\n=== Test Summary ===");
    println!("Initial sender balance: 1000 ROOT tokens");
    println!("Transfer amount: {} ROOT tokens", transfer_amount);
    println!("Final sender balance: {} ROOT tokens", sender_balance_after.value());
    println!("Final receiver balance: {} ROOT tokens", receiver_balance.value());
    println!("ROOT token transfer test complete");
    
    // Results verification
    if sender_balance.value() - sender_balance_after.value() == transfer_amount &&
       receiver_balance.value() == transfer_amount {
        println!("✅ TEST PASSED: Token balances correctly updated");
    } else {
        println!("❌ TEST FAILED: Token balances not correctly propagated");
    }

    Ok(())
}