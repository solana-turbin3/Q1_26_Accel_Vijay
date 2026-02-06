#[cfg(test)]
mod tests {

    use {
        anchor_lang::{
            prelude::msg, 
            solana_program::program_pack::Pack, 
            AccountDeserialize, 
            InstructionData, 
            ToAccountMetas
        }, anchor_spl::{
            associated_token::{
                self, 
                spl_associated_token_account
            }, 
            token::spl_token
        }, 
        litesvm::LiteSVM, 
        litesvm_token::{
            spl_token::ID as TOKEN_PROGRAM_ID, 
            CreateAssociatedTokenAccount, 
            CreateMint, MintTo
        }, 
        solana_rpc_client::rpc_client::RpcClient,
        solana_account::Account,
        solana_instruction::Instruction, 
        solana_keypair::Keypair, 
        solana_message::Message, 
        solana_native_token::LAMPORTS_PER_SOL, 
        solana_pubkey::Pubkey, 
        solana_sdk_ids::system_program::ID as SYSTEM_PROGRAM_ID, 
        solana_signer::Signer, 
        solana_transaction::Transaction, 
        solana_address::Address, 
        std::{
            path::PathBuf, 
            str::FromStr
        }
    };

    static PROGRAM_ID: Pubkey = crate::ID;

    // Setup function to initialize LiteSVM and create a payer keypair
    // Also loads an account from devnet into the LiteSVM environment (for testing purposes)
    fn setup() -> (LiteSVM, Keypair) {
        // Initialize LiteSVM and payer
        let mut program = LiteSVM::new();
        let payer = Keypair::new();
    
        // Airdrop some SOL to the payer keypair
        program
            .airdrop(&payer.pubkey(), 10 * LAMPORTS_PER_SOL)
            .expect("Failed to airdrop SOL to payer");
    
        // Load program SO file
        let so_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/deploy/anchor_escrow.so");
    
        let program_data = std::fs::read(so_path).expect("Failed to read program SO file");
    
        program.add_program(PROGRAM_ID, &program_data);

        // Example on how to Load an account from devnet
        // LiteSVM does not have access to real Solana network data since it does not have network access,
        // so we use an RPC client to fetch account data from devnet
        let rpc_client = RpcClient::new("https://api.devnet.solana.com");
        let account_address = Address::from_str("DRYvf71cbF2s5wgaJQvAGkghMkRcp5arvsK2w97vXhi2").unwrap();
        let fetched_account = rpc_client
            .get_account(&account_address)
            .expect("Failed to fetch account from devnet");

        // Set the fetched account in the LiteSVM environment
        // This allows us to simulate interactions with this account during testing
        program.set_account(payer.pubkey(), Account { 
            lamports: fetched_account.lamports, 
            data: fetched_account.data, 
            owner: Pubkey::from(fetched_account.owner.to_bytes()), 
            executable: fetched_account.executable, 
            rent_epoch: fetched_account.rent_epoch 
        }).unwrap();

        msg!("Lamports of fetched account: {}", fetched_account.lamports);
    
        // Return the LiteSVM instance and payer keypair
        (program, payer)
    }

    #[test]
    fn test_make() {

        // Setup the test environment by initializing LiteSVM and creating a payer keypair
        let (mut program, payer) = setup();

        // Get the maker's public key from the payer keypair
        let maker = payer.pubkey();
        
        // Create two mints (Mint A and Mint B) with 6 decimal places and the maker as the authority
        // This done using litesvm-token's CreateMint utility which creates the mint in the LiteSVM environment
        let mint_a = CreateMint::new(&mut program, &payer)
            .decimals(6)
            .authority(&maker)
            .send()
            .unwrap();
        msg!("Mint A: {}\n", mint_a);

        let mint_b = CreateMint::new(&mut program, &payer)
            .decimals(6)
            .authority(&maker)
            .send()
            .unwrap();
        msg!("Mint B: {}\n", mint_b);

        // Create the maker's associated token account for Mint A
        // This is done using litesvm-token's CreateAssociatedTokenAccount utility
        let maker_ata_a = CreateAssociatedTokenAccount::new(&mut program, &payer, &mint_a)
            .owner(&maker).send().unwrap();
        msg!("Maker ATA A: {}\n", maker_ata_a);

        // Derive the PDA for the escrow account using the maker's public key and a seed value
        let escrow = Pubkey::find_program_address(
            &[b"escrow", maker.as_ref(), &123u64.to_le_bytes()],
            &PROGRAM_ID
        ).0;
        msg!("Escrow PDA: {}\n", escrow);

        // Derive the PDA for the vault associated token account using the escrow PDA and Mint A
        let vault = associated_token::get_associated_token_address(&escrow, &mint_a);
        msg!("Vault PDA: {}\n", vault);

        // Define program IDs for associated token program, token program, and system program
        let asspciated_token_program = spl_associated_token_account::ID;
        let token_program = TOKEN_PROGRAM_ID;
        let system_program = SYSTEM_PROGRAM_ID;

        // Mint 1,000 tokens (with 6 decimal places) of Mint A to the maker's associated token account
        MintTo::new(&mut program, &payer, &mint_a, &maker_ata_a, 1000000000)
            .send()
            .unwrap();

        // Create the "Make" instruction to deposit tokens into the escrow
        let make_ix = Instruction {
            program_id: PROGRAM_ID,
            accounts: crate::accounts::Make {
                maker: maker,
                mint_a: mint_a,
                mint_b: mint_b,
                maker_ata_a: maker_ata_a,
                escrow: escrow,
                vault: vault,
                associated_token_program: asspciated_token_program,
                token_program: token_program,
                system_program: system_program,
            }.to_account_metas(None),
            data: crate::instruction::Make {deposit: 10, seed: 123u64, receive: 10 }.data(),
        };

        // Create and send the transaction containing the "Make" instruction
        let message = Message::new(&[make_ix], Some(&payer.pubkey()));
        let recent_blockhash = program.latest_blockhash();

        let transaction = Transaction::new(&[&payer], message, recent_blockhash);

        // Send the transaction and capture the result
        let tx = program.send_transaction(transaction).unwrap();

        // Log transaction details
        msg!("\n\nMake transaction sucessfull");
        msg!("CUs Consumed: {}", tx.compute_units_consumed);
        msg!("Tx Signature: {}", tx.signature);

        // Verify the vault account and escrow account data after the "Make" instruction
        let vault_account = program.get_account(&vault).unwrap();
        let vault_data = spl_token::state::Account::unpack(&vault_account.data).unwrap();
        assert_eq!(vault_data.amount, 10);
        assert_eq!(vault_data.owner, escrow);
        assert_eq!(vault_data.mint, mint_a);

        let escrow_account = program.get_account(&escrow).unwrap();
        let escrow_data = crate::state::Escrow::try_deserialize(&mut escrow_account.data.as_ref()).unwrap();
        assert_eq!(escrow_data.seed, 123u64);
        assert_eq!(escrow_data.maker, maker);
        assert_eq!(escrow_data.mint_a, mint_a);
        assert_eq!(escrow_data.mint_b, mint_b);
        assert_eq!(escrow_data.receive, 10);

    }

    #[test]
    fn test_take() {

        // Setup the test environment
        let (mut program, payer) = setup();
        let maker = payer.pubkey();

        // Create two mints (Mint A and Mint B) with 6 decimal places
        let mint_a = CreateMint::new(&mut program, &payer)
            .decimals(6)
            .authority(&maker)
            .send()
            .unwrap();

        let mint_b = CreateMint::new(&mut program, &payer)
            .decimals(6)
            .authority(&maker)
            .send()
            .unwrap();

        // Create maker's ATA for Mint A and mint tokens
        let maker_ata_a = CreateAssociatedTokenAccount::new(&mut program, &payer, &mint_a)
            .owner(&maker).send().unwrap();

        MintTo::new(&mut program, &payer, &mint_a, &maker_ata_a, 1_000_000_000)
            .send()
            .unwrap();

        // Derive the escrow PDA and vault ATA
        let seed: u64 = 123;
        let escrow = Pubkey::find_program_address(
            &[b"escrow", maker.as_ref(), &seed.to_le_bytes()],
            &PROGRAM_ID
        ).0;
        let vault = associated_token::get_associated_token_address(&escrow, &mint_a);

        let associated_token_program = spl_associated_token_account::ID;
        let token_program = TOKEN_PROGRAM_ID;
        let system_program = SYSTEM_PROGRAM_ID;

        // Execute the "Make" instruction to set up the escrow
        let make_ix = Instruction {
            program_id: PROGRAM_ID,
            accounts: crate::accounts::Make {
                maker,
                mint_a,
                mint_b,
                maker_ata_a,
                escrow,
                vault,
                associated_token_program,
                token_program,
                system_program,
            }.to_account_metas(None),
            data: crate::instruction::Make { deposit: 10, seed, receive: 10 }.data(),
        };

        let message = Message::new(&[make_ix], Some(&payer.pubkey()));
        let recent_blockhash = program.latest_blockhash();
        let transaction = Transaction::new(&[&payer], message, recent_blockhash);
        program.send_transaction(transaction).unwrap();

        msg!("\n\nMake transaction successful (setup for take)");

        // Create a taker keypair and airdrop SOL
        let taker_kp = Keypair::new();
        let taker = taker_kp.pubkey();
        program.airdrop(&taker, 10 * LAMPORTS_PER_SOL).expect("Failed to airdrop to taker");

        // Create taker's ATA for Mint B and mint tokens to it
        let taker_ata_b = CreateAssociatedTokenAccount::new(&mut program, &taker_kp, &mint_b)
            .owner(&taker).send().unwrap();

        MintTo::new(&mut program, &payer, &mint_b, &taker_ata_b, 1_000_000_000)
            .send()
            .unwrap();

        // Derive taker_ata_a and maker_ata_b (will be init_if_needed by program)
        let taker_ata_a = associated_token::get_associated_token_address(&taker, &mint_a);
        let maker_ata_b = associated_token::get_associated_token_address(&maker, &mint_b);

        // Execute the "Take" instruction
        let take_ix = Instruction {
            program_id: PROGRAM_ID,
            accounts: crate::accounts::Take {
                taker,
                maker,
                mint_a,
                mint_b,
                taker_ata_a,
                taker_ata_b,
                maker_ata_b,
                escrow,
                vault,
                associated_token_program,
                token_program,
                system_program,
            }.to_account_metas(None),
            data: crate::instruction::Take {}.data(),
        };

        let message = Message::new(&[take_ix], Some(&taker_kp.pubkey()));
        let recent_blockhash = program.latest_blockhash();
        let transaction = Transaction::new(&[&taker_kp], message, recent_blockhash);
        let tx = program.send_transaction(transaction).unwrap();

        msg!("\n\nTake transaction successful");
        msg!("CUs Consumed: {}", tx.compute_units_consumed);
        msg!("Tx Signature: {}", tx.signature);

        // Verify: taker received Mint A tokens from the vault
        let taker_ata_a_account = program.get_account(&taker_ata_a).unwrap();
        let taker_ata_a_data = spl_token::state::Account::unpack(&taker_ata_a_account.data).unwrap();
        assert_eq!(taker_ata_a_data.amount, 10);
        assert_eq!(taker_ata_a_data.owner, taker);

        // Verify: maker received Mint B tokens from the taker
        let maker_ata_b_account = program.get_account(&maker_ata_b).unwrap();
        let maker_ata_b_data = spl_token::state::Account::unpack(&maker_ata_b_account.data).unwrap();
        assert_eq!(maker_ata_b_data.amount, 10);
        assert_eq!(maker_ata_b_data.owner, maker);

        // Verify: vault account is closed (0 lamports)
        let vault_account = program.get_account(&vault);
        assert!(vault_account.is_none() || vault_account.unwrap().lamports == 0);

        // Verify: escrow account is closed (0 lamports)
        let escrow_account = program.get_account(&escrow);
        assert!(escrow_account.is_none() || escrow_account.unwrap().lamports == 0);

    }

    #[test]
    fn test_refund() {

        // Setup the test environment
        let (mut program, payer) = setup();
        let maker = payer.pubkey();

        // Create two mints (Mint A and Mint B) with 6 decimal places
        let mint_a = CreateMint::new(&mut program, &payer)
            .decimals(6)
            .authority(&maker)
            .send()
            .unwrap();

        let mint_b = CreateMint::new(&mut program, &payer)
            .decimals(6)
            .authority(&maker)
            .send()
            .unwrap();

        // Create maker's ATA for Mint A and mint tokens
        let maker_ata_a = CreateAssociatedTokenAccount::new(&mut program, &payer, &mint_a)
            .owner(&maker).send().unwrap();

        MintTo::new(&mut program, &payer, &mint_a, &maker_ata_a, 1_000_000_000)
            .send()
            .unwrap();

        // Derive the escrow PDA and vault ATA
        let seed: u64 = 123;
        let escrow = Pubkey::find_program_address(
            &[b"escrow", maker.as_ref(), &seed.to_le_bytes()],
            &PROGRAM_ID
        ).0;
        let vault = associated_token::get_associated_token_address(&escrow, &mint_a);

        let associated_token_program = spl_associated_token_account::ID;
        let token_program = TOKEN_PROGRAM_ID;
        let system_program = SYSTEM_PROGRAM_ID;

        // Execute the "Make" instruction to set up the escrow
        let make_ix = Instruction {
            program_id: PROGRAM_ID,
            accounts: crate::accounts::Make {
                maker,
                mint_a,
                mint_b,
                maker_ata_a,
                escrow,
                vault,
                associated_token_program,
                token_program,
                system_program,
            }.to_account_metas(None),
            data: crate::instruction::Make { deposit: 10, seed, receive: 10 }.data(),
        };

        let message = Message::new(&[make_ix], Some(&payer.pubkey()));
        let recent_blockhash = program.latest_blockhash();
        let transaction = Transaction::new(&[&payer], message, recent_blockhash);
        program.send_transaction(transaction).unwrap();

        msg!("\n\nMake transaction successful (setup for refund)");

        // Verify vault has tokens before refund
        let vault_account = program.get_account(&vault).unwrap();
        let vault_data = spl_token::state::Account::unpack(&vault_account.data).unwrap();
        assert_eq!(vault_data.amount, 10);

        // Verify maker's balance decreased after make
        let maker_ata_a_account = program.get_account(&maker_ata_a).unwrap();
        let maker_ata_a_data = spl_token::state::Account::unpack(&maker_ata_a_account.data).unwrap();
        assert_eq!(maker_ata_a_data.amount, 1_000_000_000 - 10);

        // Execute the "Refund" instruction
        let refund_ix = Instruction {
            program_id: PROGRAM_ID,
            accounts: crate::accounts::Refund {
                maker,
                mint_a,
                maker_ata_a,
                escrow,
                vault,
                token_program,
                system_program,
            }.to_account_metas(None),
            data: crate::instruction::Refund {}.data(),
        };

        let message = Message::new(&[refund_ix], Some(&payer.pubkey()));
        let recent_blockhash = program.latest_blockhash();
        let transaction = Transaction::new(&[&payer], message, recent_blockhash);
        let tx = program.send_transaction(transaction).unwrap();

        msg!("\n\nRefund transaction successful");
        msg!("CUs Consumed: {}", tx.compute_units_consumed);
        msg!("Tx Signature: {}", tx.signature);

        // Verify: maker got all tokens back
        let maker_ata_a_account = program.get_account(&maker_ata_a).unwrap();
        let maker_ata_a_data = spl_token::state::Account::unpack(&maker_ata_a_account.data).unwrap();
        assert_eq!(maker_ata_a_data.amount, 1_000_000_000);

        // Verify: vault account is closed (0 lamports)
        let vault_account = program.get_account(&vault);
        assert!(vault_account.is_none() || vault_account.unwrap().lamports == 0);

        // Verify: escrow account is closed (0 lamports)
        let escrow_account = program.get_account(&escrow);
        assert!(escrow_account.is_none() || escrow_account.unwrap().lamports == 0);

    }

}