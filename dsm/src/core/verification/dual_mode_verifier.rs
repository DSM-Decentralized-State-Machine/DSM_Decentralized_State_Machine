// Consolidated DualModeVerifier implementation based on whitepaper Section 30
// Handles both bilateral V(Sn,Sn+1,σA,σB) and unilateral Vuni(Sn,Sn+1,σA,Dverify(IDB)) modes.

use crate::core::entropy::DeterministicEntropy;
use crate::types::error::DsmError;
use crate::types::operations::{Operation, TransactionMode};
use crate::types::state_types::{State, PreCommitment};
use crate::types::token_types::Balance;
use crate::crypto;

/// DualModeVerifier implements the verification predicates from whitepaper Section 30
pub struct DualModeVerifier;

impl DualModeVerifier {
    /// Verify a state transition according to its mode and verification type
    pub fn verify_transition(
        current_state: &State,
        next_state: &State,
        operation: &Operation,
    ) -> Result<bool, DsmError> {
        // Get mode-specific validation logic
        match operation {
            Operation::Transfer { mode, .. } => {
                match mode {
                    TransactionMode::Bilateral => {
                        // V(Sn,Sn+1,σA,σB) = true
                        Self::verify_bilateral_mode(current_state, next_state)
                    },
                    TransactionMode::Unilateral => {
                        // Vuni(Sn,Sn+1,σA,Dverify(IDB)) = true 
                        Self::verify_unilateral_mode(current_state, next_state)
                    }
                }
            },
            Operation::RemoveRelationship { .. } => {
                // For remove relationship, use basic transition verification
                Self::verify_basic_transition(current_state, next_state)
            },
            _ => Self::verify_basic_transition(current_state, next_state)
        }
    }

    /// Verify bilateral mode transition according to whitepaper equation (87)
    fn verify_bilateral_mode(
        current_state: &State,
        next_state: &State,
    ) -> Result<bool, DsmError> {
        // 1. Verify both signatures exist
        if next_state.entity_sig.is_none() || 
           next_state.counterparty_sig.is_none() {
            return Ok(false); 
        }

        // 2. Verify signatures are valid for state transition
        if !Self::verify_signatures(current_state, next_state)? {
            return Ok(false);
        }

        // 3. Verify state transition preserves invariants
        Self::verify_transition_invariants(current_state, next_state)
    }

    /// Verify unilateral mode transition according to whitepaper equation (88)
    fn verify_unilateral_mode(
        current_state: &State,
        next_state: &State,
    ) -> Result<bool, DsmError> {
        // 1. Verify sender signature
        if next_state.entity_sig.is_none() {
            return Ok(false);
        }

        // 2. Verify sender signature is valid
        if !Self::verify_entity_signature(current_state, next_state)? {
            return Ok(false);
        }

        // 3. Verify recipient identity anchor exists in decentralized storage
        if !Self::verify_recipient_identity(next_state)? {
            return Ok(false);
        }

        // 4. Verify state transition preserves invariants
        Self::verify_transition_invariants(current_state, next_state)
    }

    /// Verify a batch of transitions
    pub fn verify_transition_batch(states: &[State]) -> Result<bool, DsmError> {
        if states.len() < 2 {
            return Ok(true); // Nothing to verify with 0 or 1 states
        }

        // Verify each pair of consecutive states
        for i in 0..(states.len() - 1) {
            let prev = &states[i];
            let next = &states[i + 1];

            if !Self::verify_transition(prev, next, &next.operation)? {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Verify basic transition properties common to all operations
    fn verify_basic_transition(
        current_state: &State,
        next_state: &State,
    ) -> Result<bool, DsmError> {
        // Delegate to transition invariants verification
        Self::verify_transition_invariants(current_state, next_state)
    }

    fn verify_transition_invariants(
        current_state: &State,
        next_state: &State,
    ) -> Result<bool, DsmError> {
        // 1. Verify state number monotonically increases
        if next_state.state_number != current_state.state_number + 1 {
            return Ok(false);
        }

        // 2. Verify hash chain continuity
        if next_state.prev_state_hash != current_state.hash()? {
            return Ok(false);
        }

        // 3. Verify token conservation
        if !Self::verify_token_conservation(current_state, next_state)? {
            return Ok(false);
        }

        // 4. Verify entropy evolution using the consolidated implementation
        if !Self::verify_entropy_evolution(current_state, next_state)? {
            return Ok(false);
        }

        Ok(true)
    }

    fn verify_signatures(
        _current_state: &State,
        next_state: &State,
    ) -> Result<bool, DsmError> {
        // Verify both parties' signatures on state transition
        if let (Some(entity_sig), Some(counterparty_sig)) = 
            (&next_state.entity_sig, &next_state.counterparty_sig) {
            
            // Get the state data for verification
            // Compute data for signing (hash of state + metadata)
            let mut state_data = next_state.hash()?.to_vec();
            if let Some(data) = next_state.get_parameter("signing_metadata") {
                state_data.extend_from_slice(data);
            }
            
            // Verify entity signature
            if !crypto::verify_signature(
                &state_data, 
                entity_sig, 
                &next_state.device_info.public_key
            ) {
                return Ok(false);
            }
            
            // Verify counterparty signature if relationship exists
            if let Some(relationship) = &next_state.relationship_context {
                if !crypto::verify_signature(
                    &state_data,
                    counterparty_sig,
                    &relationship.counterparty_public_key
                ) {
                    return Ok(false);
                }
                
                Ok(true)
            } else {
                Ok(false) // No relationship context for bilateral mode
            }
        } else {
            Ok(false) // Missing signatures
        }
    }

    fn verify_entity_signature(
        _current_state: &State,
        next_state: &State,
    ) -> Result<bool, DsmError> {
        if let Some(signature) = &next_state.entity_sig {
            // Compute data for signing (hash of state + metadata)
            let mut state_data = next_state.hash()?.to_vec();
            if let Some(data) = next_state.get_parameter("signing_metadata") {
                state_data.extend_from_slice(data);
            }
            
            Ok(crypto::verify_signature(
                &state_data,
                signature,
                &next_state.device_info.public_key
            ))
        } else {
            Ok(false)
        }
    }
     
    /// Verify recipient identity in decentralized storage
    fn verify_recipient_identity(state: &State) -> Result<bool, DsmError> {
        // In a real implementation, this would check with decentralized storage
        // For now, we just check if the relationship context contains valid data
        if let Some(relationship) = &state.relationship_context {
            if relationship.counterparty_id.is_empty() || 
               relationship.counterparty_public_key.is_empty() {
                Ok(false)
            } else {
                Ok(true)
            }
        } else {
            // For non-relationship operations, this is still valid
            Ok(true)
        }
    }

    fn verify_token_conservation(
        current_state: &State,
        next_state: &State,
    ) -> Result<bool, DsmError> {
        // Verify token balances are conserved according to operation type
        for (token_id, current_balance) in &current_state.token_balances {
            match next_state.token_balances.get(token_id) {
                Some(next_balance) => {
                    // Balance changes must be justified by the operation
                    if current_balance.value() != next_balance.value() {
                        // Verify change is valid according to operation
                        if !Self::verify_balance_change_validity(
                            current_state,
                            next_state,
                            token_id,
                            current_balance.value(),
                            next_balance.value(),
                        )? {
                            return Ok(false);
                        }
                    }
                },
                None => {
                    // Token must still exist unless explicitly removed
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
  
    fn verify_balance_change_validity(
        current_state: &State,
        next_state: &State,
        token_id: &str,
        current_balance: u64,
        next_balance: u64,
    ) -> Result<bool, DsmError> {
        match &next_state.operation {
            Operation::Transfer { amount, token_id: op_token_id, .. } => {
                // Verify token ID matches
                if token_id != op_token_id {
                    return Ok(false);
                }
                
                // Verify transfer amount matches balance change
                let amount_value = amount.value();
                if next_balance != current_balance - amount_value as u64 {
                    return Ok(false);
                }
                
                // Verify transfer is valid
                Self::verify_transfer_validity(current_state, next_state, token_id, amount)
            },
            _ => Ok(false),
        }
    }

    fn verify_entropy_evolution(
        current_state: &State,
        next_state: &State,
    ) -> Result<bool, DsmError> {
        // Verify entropy evolution using the deterministic entropy function
        let new_entropy = DeterministicEntropy::derive_entropy(
            &current_state.entropy,
            &next_state.operation,
            current_state.state_number + 1,
        )?;
        
        if new_entropy != next_state.entropy {
            return Ok(false);
        }
        
        Ok(true)
    }

    #[allow(dead_code)]
    fn verify_precommitment_adherence(
        commitment: &PreCommitment,
        next_state: &State,
    ) -> Result<bool, DsmError> {
        // Verify pre-commitment conditions are met using available commitment fields
        if next_state.state_number != commitment.min_state_number + 1 {
            return Ok(false);
        }
        
        if next_state.prev_state_hash != commitment.hash {
            return Ok(false);
        }
        
        // Removed entropy check because PreCommitment does not include an entropy field
        Ok(true)
    }

    /// Verify transfer validity
    /// This function checks if a transfer operation is valid based on the current and next state.
    /// It ensures that the transfer adheres to the rules defined for token transfers.
    fn verify_transfer_validity(
        current_state: &State,
        _next_state: &State,
        token_id: &str,
        amount: &Balance,
    ) -> Result<bool, DsmError> {
        // Check if the token ID exists in the current state
        if !current_state.token_balances.contains_key(token_id) {
            return Ok(false);
        }

        // Check if the amount is valid (greater than zero)
        if amount.value() == 0 {
            return Ok(false);
        }

        // Check if the transfer amount does not exceed the current balance
        let current_balance = current_state.token_balances[token_id].value();
        if amount.value() > current_balance {
            return Ok(false);
        }

        Ok(true)
    }
}
