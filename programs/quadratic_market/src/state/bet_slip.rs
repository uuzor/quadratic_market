use anchor_lang::prelude::*;

/// Input struct for slip legs - used when placing a slip
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, Default, Copy)]
pub struct SlipLeg {
    pub market_id: u64,
    pub outcome_id: u8,
    pub num_shares: u64,
}
