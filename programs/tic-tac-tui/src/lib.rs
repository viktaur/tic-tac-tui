use anchor_lang::prelude::*;
use state::GameState;

// Specifies the program's on-chain address.
declare_id!("11111111111111111111111111111111");

const DISCRIMINATOR_LEN: usize = 8;
const TIMEOUT_SLOTS: u64 = 150;

mod state;

/// Contains program's instruction logic
#[program]
mod tic_tac_tui {
    use super::*;
    use anchor_lang::system_program;

    /// Player 1 creates game passing a randomly client-generated u64 game ID.
    ///
    /// Status: _ -> AwaitingPlayerToJoinGame
    pub fn initialize_game(
        ctx: Context<InitializeGame>,
        game_id: u64,
        wager: u64
    ) -> Result<()> {
        let game = &mut ctx.accounts.game_state;
        game.player1 = ctx.accounts.signer.key();
        game.player2 = None;
        game.p1_score = 0;
        game.p2_score = 0;
        game.status = GameStatus::AwaitingPlayerToJoinGame;
        game.game_id = game_id;
        game.wager_per_round = wager;
        game.round = 0;
        game.bump = ctx.bumps.game_state;
        Ok(())
    }

    /// Called by player 2 to join the game providing the game ID.
    ///
    /// Status: AwaitingPlayerToJoinGame -> BetweenRounds
    pub fn join_game(ctx: Context<GameStateUpdate>, _game_id: u64) -> Result<()> {
        let game = &mut ctx.accounts.game_state;
        let signer = &ctx.accounts.signer;
        require!(game.player1 != signer.key(), GameError::InvalidAction);
        require!(game.status == GameStatus::AwaitingPlayerToJoinGame, GameError::StaleState);

        game.player2 = Some(signer.key());
        game.status = GameStatus::BetweenRounds;
        Ok(())
    }

    /// A player signals they are ready to play. This needs to be called by both players
    /// in order for the round to start.
    ///
    /// Status:
    ///     BetweenRounds -> BetweenRounds | RoundActive
    pub fn ready_up(ctx: Context<GameStateUpdate>, _game_id: u64) -> Result<()> {
        let game = &mut ctx.accounts.game_state;
        let signer = &ctx.accounts.signer;
        let player_1 = game.player1;
        let player_2 = {
            require!(game.player2.is_some(), GameError::MissingPlayer);
            game.player2.expect("Player 2 should be present")
        };
        let wager = game.wager_per_round;

        // Transfer wager from signer into escrow
        let pay_wager = || {
            system_program::transfer(
                CpiContext::new(
                    ctx.accounts.system_program.to_account_info(),
                    system_program::Transfer {
                        from: signer.to_account_info(),
                        to: game.to_account_info()
                    }
                ),
                wager
            )
        };

        require!(signer.key() == player_1 || signer.key() == player_2,
            GameError::Unauthorized
        );
        require!(game.status == GameStatus::BetweenRounds, GameError::InvalidAction);

        match (game.p1_ready, game.p2_ready) {
            (false, false) => {
                pay_wager()?;
                if signer.key() == player_1 {
                    game.p1_ready = true;
                } else {
                    game.p2_ready = true;
                }
            },
            (true, false) => {
                require!(signer.key() == player_2, GameError::InvalidAction);
                pay_wager()?;
                game.p2_ready = true;
                game.start_round()?;
            },
            (false, true) => {
                require!(signer.key() == player_1, GameError::InvalidAction);
                pay_wager()?;
                game.p1_ready = true;
                game.start_round()?;
            },
            (true, true) => unreachable!(
                "Both players being ready should not have passed the games status check"
            )
        }

        Ok(())
    }

    /// A player cancels their ready-up signal, getting back the money from escrow.
    ///
    /// Status: BetweenRounds -> BetweenRounds
    pub fn cancel_ready_up(ctx: Context<GameStateUpdate>, _game_id: u64) -> Result<()> {
        let game = &mut ctx.accounts.game_state;
        let signer = &ctx.accounts.signer;
        let player_1 = game.player1;
        let player_2 = {
            require!(game.player2.is_some(), GameError::MissingPlayer);
            game.player2.expect("Player 2 should be present")
        };
        let wager = game.wager_per_round;

        let refund_wager = |game: &mut Account<'_, GameState>| -> Result<()> {
            **game.to_account_info().try_borrow_mut_lamports()? -= wager;
            **signer.to_account_info().try_borrow_mut_lamports()? += wager;
            Ok(())
        };

        require!(signer.key() == player_1 || signer.key() == player_2,
            GameError::Unauthorized
        );
        require!(game.status == GameStatus::BetweenRounds, GameError::InvalidAction);

        match (game.p1_ready, game.p2_ready) {
            (false, false) => { return Err(GameError::InvalidAction.into()); },
            (true, false) => {
                require!(signer.key() == player_1, GameError::InvalidAction);
                game.p1_ready = false;
                refund_wager(game)?;
            },
            (false, true) => {
                require!(signer.key() == player_2, GameError::InvalidAction);
                game.p2_ready = false;
                refund_wager(game)?
            },
            (true, true) => unreachable!(
                "Both players being ready should not have passed the games status check"
            )
        }

        Ok(())
    }

    /// Either player makes a move on the game board. Validates turn, updates board,
    /// checks for a completed row or draw, and updates game status if finalised.
    ///
    /// `cell` should be a number between 0 and 8.
    ///
    /// Status: RoundActive -> RoundActive | BetweenRounds
    pub fn make_move(ctx: Context<GameStateUpdate>, _game_id: u64, cell: u8) -> Result<()> {
        let game = &mut ctx.accounts.game_state;
        let signer = &ctx.accounts.signer;
        let player_1 = game.player1;
        let player_2 = {
            require!(game.player2.is_some(), GameError::MissingPlayer);
            game.player2.expect("Player 2 should be present")
        };

        require!(signer.key() == player_1 || signer.key() == player_2,
            GameError::Unauthorized
        );
        require!(game.status == GameStatus::RoundActive, GameError::InvalidAction);

        // Check if it's the player's turn
        match game.turn {
            1 => require!(
                ctx.accounts.signer.key() == game.player1,
                GameError::PlayerWrongTurn
            ),
            2 => require!(
                Some(ctx.accounts.signer.key()) == game.player2,
                GameError::PlayerWrongTurn
            ),
            _ => unreachable!()
        };

        // Check move is valid and update the cell
        require!(cell < 9 && game.board[cell as usize] == 0, GameError::InvalidGameMove);
        let turn = game.turn;
        game.apply_move(cell, turn);

        // Check the board state after move, and end the round with the corresponding result
        let round_result = game.check_board(cell, game.turn);

        // Update state before potential lamport transfer
        game.move_count += 1;
        game.last_move_slot = Clock::get()?.slot;
        game.turn = if game.turn == 1 { 2 } else { 1 };

        if let Some(result) = round_result {
            match result {
                RoundResult::Player1Won | RoundResult::Player2Won => {
                    // The player who just moved is the winner (they're the signer).
                    // Pay out the full accumulated escrow (wagers from all rounds including draws).
                    let escrow = escrow_balance(&game.to_account_info())?;
                    **game.to_account_info().try_borrow_mut_lamports()? -= escrow;
                    **ctx.accounts.signer.to_account_info().try_borrow_mut_lamports()? += escrow;
                }
                RoundResult::Draw => {
                    // Wager stays in escrow and accumulates into the next round.
                }
            }

            // Update state after round has finalised
            game.end_round(result.clone())?;
        }

        Ok(())
    }

    /// Claims a round as won if a timeout is hit, claiming the money on escrow.
    ///
    /// Status: RoundActive -> BetweenRounds
    pub fn claim_timeout(ctx: Context<ClaimTimeout>, _game_id: u64) -> Result<()> {
        let game = &mut ctx.accounts.game_state;
        let signer = &ctx.accounts.signer;
        let last_move_slot = game.last_move_slot;
        let current_slot = Clock::get()?.slot;

        require!(
            current_slot
                .checked_sub(last_move_slot)
                .ok_or_else(|| error!(GameError::StaleState))? > TIMEOUT_SLOTS,
            GameError::StaleState
        );

        // Award win to the signer (the waiting player whose opponent timed out).
        // They receive the full accumulated escrow.
        let result = if signer.key() == game.player1 {
            RoundResult::Player1Won
        } else {
            RoundResult::Player2Won
        };
        let escrow = escrow_balance(&game.to_account_info())?;
        game.end_round(result)?;

        **game.to_account_info().try_borrow_mut_lamports()? -= escrow;
        **signer.to_account_info().try_borrow_mut_lamports()? += escrow;

        Ok(())
    }

    /// This is called unilaterally whenever one of the players wants to end the game. It
    /// will then mark the game as invalid and any money on escrow will be returned 50/50
    /// to the players. Cannot be called while a round is active.
    ///
    /// Status: BetweenRounds | Player1Ready | Player2Ready | AwaitingPlayerToJoinGame -> Terminated
    pub fn terminate_game(ctx: Context<TerminateGame>, _game_id: u64) -> Result<()> {
        let game = &mut ctx.accounts.game_state;
        require!(game.status != GameStatus::RoundActive, GameError::StaleState);

        game.status = GameStatus::Terminated;

        let escrow = escrow_balance(&game.to_account_info())?;
        if escrow > 0 {
            let each = escrow / 2;
            let remainder = escrow % 2;
            **game.to_account_info().try_borrow_mut_lamports()? -= escrow;
            **ctx.accounts.player1.try_borrow_mut_lamports()? += each + remainder;
            **ctx.accounts.player2.try_borrow_mut_lamports()? += each;
        }

        Ok(())
    }
}

fn escrow_balance(account: &AccountInfo) -> Result<u64> {
    let rent = Rent::get()?;
    let min_rent = rent.minimum_balance(account.data_len());
    Ok(account.lamports().saturating_sub(min_rent))
}

// This needs to be separate because the account is initialized.
#[derive(Accounts)]
#[instruction(game_id: u64)]
pub struct InitializeGame<'info> {
    #[account(
        init,
        payer = signer,
        space = DISCRIMINATOR_LEN + GameState::INIT_SPACE,
        seeds = [b"tic_tac_toe", game_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub game_state: Account<'info, GameState>,
    #[account(mut)]
    pub signer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(game_id: u64)]
pub struct GameStateUpdate<'info> {
    #[account(
        mut,
        seeds = [b"tic_tac_toe", game_id.to_le_bytes().as_ref()],
        bump = game_state.bump,
    )]
    pub game_state: Account<'info, GameState>,
    #[account(mut)]
    pub signer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(game_id: u64)]
pub struct TerminateGame<'info> {
    #[account(
        mut,
        seeds = [b"tic_tac_toe", game_id.to_le_bytes().as_ref()],
        bump = game_state.bump,
    )]
    pub game_state: Account<'info, GameState>,
    #[account(mut, address = game_state.player1)]
    pub player1: SystemAccount<'info>,
    #[account(mut, address = game_state.player2.ok_or(GameError::MissingPlayer)?)]
    pub player2: SystemAccount<'info>,
    pub signer: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(game_id: u64)]
pub struct ClaimTimeout<'info> {
    #[account(
        mut,
        seeds = [b"tic_tac_toe", game_id.to_le_bytes().as_ref()],
        bump = game_state.bump,
    )]
    pub game_state: Account<'info, GameState>,
    #[account(mut)]
    pub signer: Signer<'info>,
}

#[derive(InitSpace, AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq)]
pub enum GameStatus {
    /// Player 1 has created the game and we're awaiting player 2 to join.
    AwaitingPlayerToJoinGame,
    /// A round has finished and we're waiting for the next one to start.
    BetweenRounds,
    /// A round is currently in progress.
    RoundActive,
    /// The game is over, accounts can be closed.
    Terminated,
}

#[derive(InitSpace, AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq)]
pub enum RoundResult {
    Player1Won,
    Player2Won,
    Draw,
}

#[error_code]
pub enum GameError {
    #[msg("Player is not allowed to perform an action at this time")]
    InvalidAction,
    #[msg("Player is not allowed to make a move at this time")]
    PlayerWrongTurn,
    #[msg("Player 2 is missing from the game state")]
    MissingPlayer,
    #[msg("Game move is not valid (`cell` must be an integer between 1 and 9 and not already played)")]
    InvalidGameMove,
    #[msg("Signer does not have permission to perform actions in this game")]
    Unauthorized,
    #[msg("The game state is stale and does not match the expected state for this action")]
    StaleState,
}
