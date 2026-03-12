use anchor_lang::prelude::{borsh::BorshDeserialize, *};

/// Specifies the program's on-chain address.
declare_id!("11111111111111111111111111111111");

const DISCRIMINATOR_LEN: usize = 8;

/// Contains program's instruction logic
#[program]
mod tic_tac_tui {
    use super::*;

    /// Player 1 creates game and deposit tokens.
    pub fn initialize_game(ctx: Context<InitializeGame>, wager: u64) -> Result<()> {
        // Set AwaitingPlayer2
        todo!()
    }

    /// Player 2 joins
    pub fn join_game(ctx: Context<JoinGame>, ) -> Result<()> {
        // Set to BetweenRounds
        todo!()
    }

    pub fn start_round() -> Result<()> {
        todo!()
    }

    /// Either player makes a move on the game board. Validates turn, updates board,
    /// checks for a completed row or draw, and updates game status if finalised.
    pub fn make_move(ctx: Context<MakeMove>, ) -> Result<()> {
        todo!()
    }

    pub fn terminate_game(ctx: Context<CancelGame>, ) -> Result<()> {
        todo!()
    }

    pub fn claim_timeout(ctx: Context<ClaimTimeout>, ) -> Result<()> {
        todo!()
        // if `current_slot - last_move_slot > TIMEOUT_SLOTS` award the win to the last-
        // moving player.
    }
}

#[derive(Accounts)]
pub struct InitializeGame<'info> {
    #[account(init, payer = signer, space = DISCRIMINATOR_LEN + GameState::INIT_SPACE)]
    pub game_state: Account<'info, GameState>,
    #[account(mut)]
    pub signer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[account]
#[derive(InitSpace)]
pub struct GameState {
    player1: Pubkey,
    player2: Pubkey,
    board: [u8; 9],     // 0=empty, 1=X, 2=O
    turn: u8,           // 1 or 2
    round: u16,
    status: GameStatus,
    last_round_result: Option<RoundResult>,
    wager_per_round: u64,
    /// Used for letting one of the players end the round and claim the money if the other
    /// player takes too long:
    last_move_slot: u64,
    /// The game escrow is a PDA, we store the bump so the program doesn't need to
    /// recalculate it every time and so we save compute units.
    escrow_bump: u8,
}

#[derive(InitSpace, AnchorSerialize, AnchorDeserialize, Clone, PartialEq)]
pub enum GameStatus {
    /// Player 1 has created the game and we're awaiting player 2 to join.
    AwaitingPlayer2,
    /// A round is currently in progress.
    RoundActive,
    /// A round has finished and we're waiting for the next one to start.
    BetweenRounds,
    /// The game is over, accounts can be closed.
    Terminated,
}

impl GameStatus {
    pub const LEN: usize = 0;
}

#[derive(InitSpace, AnchorSerialize, AnchorDeserialize, Clone, PartialEq)]
pub enum RoundResult {
    Player1Won,
    Player2Won,
    Draw,
}
