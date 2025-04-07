//! Test to verify that the hash chain implementation correctly follows the mathematical model
//! from the whitepaper, specifically the formula: S(n+1).prev_hash = H(S(n))

use dsm::core::state_machine::hashchain::HashChain;
use dsm::state_machine::hashchain::BatchStatus;
use dsm::types::error::DsmError;
use dsm::types::operations::Operation;
use dsm::types::state_types::{DeviceInfo, SparseIndex, State, StateParams};

#[test]
fn test_hash_chain_mathematical_model() -> Result<(), DsmError> {
    // Create a new hash chain
    let mut chain = HashChain::new();

    // Create a device info for testing
    let device_info = DeviceInfo::new("test_device", vec![1, 2, 3, 4]);

    // Create a genesis state
    let mut genesis = State::new_genesis(vec![1, 2, 3], device_info.clone());

    // Set ID
    genesis.id = "state_0".to_string();

    // Compute hash
    let genesis_hash = genesis.compute_hash()?;
    genesis.hash = genesis_hash.clone();

    // Add genesis state to chain
    chain.add_state(genesis.clone())?;

    // Create 10 subsequent states to verify the mathematical model
    let mut prev_state = genesis;

    for i in 1..10 {
        // Calculate sparse indices including both genesis and direct predecessor
        let mut indices = State::calculate_sparse_indices(i)?;
        if !indices.contains(&(i - 1)) {
            indices.push(i - 1);
            indices.sort_unstable();
        }
        let sparse_index = SparseIndex::new(indices);

        // Use the proper StateParams::new constructor with 8 parameters, not 9
        let operation = Operation::Generic {
            operation_type: "init".to_string(),
            data: vec![],
            message: "".to_string(),
        };

        let state_params = StateParams::new(
            i,      // state_number
            vec![], // entropy
            operation,
            device_info.clone(), // device_info
        )
        .with_encapsulated_entropy(vec![])
        .with_prev_state_hash(prev_state.hash.clone())
        .with_sparse_index(sparse_index);

        // Initialize state
        let mut state = State::new(state_params);
        state.id = format!("state_{}", i);

        // Compute and set the hash
        let hash = state.compute_hash()?;
        state.hash = hash;

        // Verify S(n+1).prev_hash = H(S(n)) - this is the key mathematical formula from the whitepaper
        assert_eq!(
            state.prev_state_hash,
            prev_state.hash,
            "Mathematical model violation: S({}).prev_hash != H(S({}))",
            i,
            i - 1
        );

        // Add state to chain
        chain.add_state(state.clone())?;

        // Update prev_state for next iteration
        prev_state = state;
    }

    // Verify the entire chain
    let chain_valid = chain.verify_chain()?;
    assert!(chain_valid, "Chain verification failed");

    // Test batch operations to verify they maintain mathematical constraints
    let batch_id = chain.create_batch()?;

    // Create a transition using the available constructors rather than direct struct construction
    let operation = Operation::Generic {
        operation_type: "batch_test".to_string(),
        data: vec![],
        message: "Batch operation test".to_string(),
    };

    // Get the current state to use for transition
    let current_state = chain.get_latest_state()?;

    // Create a state transition by using the proper factory method with only 3 required arguments
    let transition = dsm::core::state_machine::transition::create_transition(
        current_state,
        operation,
        &vec![10, 11, 12], // new_entropy
    )?;

    // Add transition to batch
    let _transition_index = chain.add_transition_to_batch(batch_id, transition.clone())?;

    // Finalize and commit batch
    chain.finalize_batch(batch_id)?;
    chain.commit_batch(batch_id)?;

    // Verify chain is still valid after batch operations
    let chain_still_valid = chain.verify_chain()?;
    assert!(
        chain_still_valid,
        "Chain verification failed after batch operations"
    );

    // Verify that the batch status is committed
    let batch_status = chain.get_batch_status(batch_id)?;
    assert_eq!(
        batch_status,
        BatchStatus::Committed,
        "Batch should be in committed status"
    );
    Ok(())
}
