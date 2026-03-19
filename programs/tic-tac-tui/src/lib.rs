use anchor_lang::prelude::{borsh::BorshDeserialize, *};

// Specifies the program's on-chain address.
declare_id!("11111111111111111111111111111111");

const DISCRIMINATOR_LEN: usize = 8;
const TIMEOUT_SLOTS: u64 = 100;

/// Contains program's instruction logic
#[program]
mod tic_tac_tui {
    use std::ops::Deref;

    use anchor_lang::system_program;

    use super::*;

    /// Player 1 creates game passing a randomnly client-generated u64 game ID.
    ///
    /// Status: _ -> AwaitingPlayerToJoinGame
    pub fn initialize_game(ctx: Context<InitializeGame>, game_id: u64, wager: u64) -> Result<()> {
        let game = &mut ctx.accounts.game_state;
        game.player1 = ctx.accounts.signer.key();
        game.player2 = None;
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
        require!(game.player1 != signer.key(), GameError::ActionWrongPlayer);
        require!(game.status == GameStatus::AwaitingPlayerToJoinGame, GameError::InvalidInstruction);

        game.player2 = Some(signer.key());
        game.status = GameStatus::BetweenRounds;
        Ok(())
    }

    /// A player signals they are ready to play. This needs to be called by both players
    /// in order for the round to start.
    ///
    /// Status:
    /// 	BetweenRounds -> BetweenRounds | RoundActive
    pub fn ready_up(ctx: Context<GameStateUpdate>, _game_id: u64) -> Result<()> {
      	let game = &mut ctx.accounts.game_state;
      	let signer = &ctx.accounts.signer;
     	let player_1 = game.player1;
     	let player_2 = {
     		require!(game.player2.is_some(), GameError::MissingPlayer);
      		game.player2.expect("Player 2 should be present")
      	};

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
         		game.wager_per_round
	       	)
      	};

  		require!(game.status == GameStatus::BetweenRounds, GameError::InvalidInstruction);

       	match (game.p1_ready, game.p2_ready) {
        	(false, false) => {
         		require!(
           			signer.key() == player_1 || signer.key() == player_2,
              		GameError::Unauthorized
           		);
          		pay_wager()?;
           		if signer.key() == player_1 {
           			game.p1_ready = true;
           		} else {
           			game.p2_ready = true;
           		}
         	},
         	(true, false) => {
           		require!(signer.key() == player_2, GameError::ActionWrongPlayer);
           		pay_wager()?;
             	game.p2_ready = true;
             	game.status = GameStatus::RoundActive;
          	},
          	(false, true) => {
         		require!(signer.key() == player_1, GameError::ActionWrongPlayer);
         		pay_wager()?;
          		game.p1_ready = true;
             	game.status = GameStatus::RoundActive;
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
    pub fn cancel_ready_up(ctx: Context<GameStateUpdate>) -> Result<()> {
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

       	require!(game.status == GameStatus::BetweenRounds, GameError::InvalidInstruction);

     	match (game.p1_ready, game.p2_ready) {
      		(false, false) => { return Err(GameError::InvalidInstruction.into()); },
         	(true, false) => {
        		require!(signer.key() == player_1, GameError::ActionWrongPlayer);
          		game.p1_ready = false;
           		refund_wager(game)?;
          	},
           	(false, true) => {
            	require!(signer.key() == player_2, GameError::ActionWrongPlayer);
             	game.p2_ready = false;
             	refund_wager(game)?
            },
            (true, true) => unreachable!("
           		Both players being ready should not have passed the games status check
            ")
      	}

        Ok(())
    }


    /// Either player makes a move on the game board. Validates turn, updates board,
    /// checks for a completed row or draw, and updates game status if finalised.
    ///
    /// `cell` should be a number between 0 and 8.
    ///
    /// Status: RoundActive -> RoundActive | BetweenRounds
    pub fn make_move(ctx: Context<GameStateUpdate>, _game_id: u8, cell: u8) -> Result<()> {
        let game = &mut ctx.accounts.game_state;
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
        require!(cell < 9, GameError::InvalidGameMove);
        require!(game.board[cell as usize] == 0, GameError::InvalidGameMove);
        game.board[cell as usize] = game.turn;

        // Check the board state after move, and
        let round_result = game.check_board(cell, game.turn);
        if let Some(result) = round_result {

            game.status = GameStatus::BetweenRounds;
            game.last_round_result = Some(result);
        }

        // Update state
        game.move_count += 1;
        game.turn = if game.turn == 1 { 2 } else { 1 };

        Ok(())
    }

    /// Claims a round as won if a timeout is hit, claiming the money on escrow.
    ///
    /// Status: BetweenRounds -> Player1Won | Player2Won
    pub fn claim_timeout(ctx: Context<ClaimTimeout>, _game_id: u8) -> Result<()> {
        // if `current_slot - last_move_slot > TIMEOUT_SLOTS` award the win to the last-
        // moving player.
        let game = &mut ctx.accounts.game_state;


        Ok(())
    }

    /// This is called unilaterally whenever one of the players wants to end the game. It
    /// will then mark the game as invalid and any money on escrow will be returned 50/50
    /// to the players. Cannot be called while a round is active.
    ///
    /// Status: BetweenRounds | Player1Ready | Player2Ready | AwaitingPlayerToJoinGame -> Terminated
    pub fn terminate_game(ctx: Context<GameStateUpdate>) -> Result<()> {
        let game = &mut ctx.accounts.game_state;

        Ok(())
    }
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

// TODO add more fields
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

#[account]
#[derive(InitSpace)]
pub struct GameState {
    player1: Pubkey,
    player2: Option<Pubkey>,
    // These two are useful at the beginning of each round
    p1_ready: bool,
    p2_ready: bool,
    board: [u8; 9],     // 0=empty, 1=X, 2=O
    move_count: u8,
    turn: u8,           // 1 or 2
    round: u16,
    status: GameStatus,
    last_round_result: Option<RoundResult>,
    wager_per_round: u64,
    // Randomly generate by the client
    game_id: u64,
    /// Used for letting one of the players end the round and claim the money if the other
    /// player takes too long:
    last_move_slot: u64,
    /// The games state information will be stored in a PDA. We store the bump so the
    /// program doesn't need to recalculate it every time.
    bump: u8,
}

impl GameState {
	 pub fn start_round(&mut self) -> Result<()> {
        self.status = GameStatus::RoundActive;
        self.round += 1;
        self.board = [0; 9];
        self.turn = 1; // Player 1 starts by default

        Ok(())
    }

    /// Checks whether the last move resulted in a row or draw. Passing over the board
    /// coordinates and the player helps massively reduce the search space.
    pub fn check_board(&self, cell: u8, player: u8) -> Option<RoundResult> {
		let row = (cell / 3) as usize;
		let col = (cell % 3) as usize;

		fn player_won(player: u8) -> Option<RoundResult> {
			match player {
     			1 => return Some(RoundResult::Player1Won),
     			2 => return Some(RoundResult::Player2Won),
     			_ => unreachable!(),
  			}
		}

		// check col
  		for i in 0..3 {
  			if self.board[3*i + col] != player {
     			break;
       		}
         	if i == 2 {
          		return player_won(player);
         	}
       	}

        // check row
        for j in 0..3 {
        	if self.board[3*row + j] != player {
        		break;
        	}
         	if j == 2 {
        		return player_won(player);
          	}
        }

        // check diagonal
        if row == col {
        	// we're on a diagonal
         	for d in 0..3 {
         		if self.board[3*d + d] != player {
           			break;
           		}
             	if d == 2 {
            		return player_won(player);
              	}
          	}
        }

        // check anti-diagonal
        if row + col == 2 {
        	// we're on an anti-diagonal
        	for d in 0..3 {
         		if self.board[3*d + (2-d)] != player {
         			break;
         		}
         		if d == 2 {
         			return player_won(player);
         		}
        	}
        }

        // check for draw
        if self.move_count == 8 {
      		return Some(RoundResult::Draw);
        }

        None
    }
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
    #[msg("Player is not allowed to perform an action again, as it expected to be performed by the other player")]
    ActionWrongPlayer,
    #[msg("Player is not allowed to make a move at this time")]
    PlayerWrongTurn,
    #[msg("Player 2 is missing from the game state")]
    MissingPlayer,
    #[msg("Player cannot perform this action from the current state")]
    InvalidInstruction,
    #[msg("Game move is not valid (`cell` must be an integer between 1 and 9 and not already played)")]
    InvalidGameMove,
    #[msg("Signer does not have permission to perform actions in this game")]
    Unauthorized,
    #[msg("The game state is stale and does not match the expected state for this action")]
    StaleState,
}
