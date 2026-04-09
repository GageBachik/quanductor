Quanductor — Full Flow                                                                    
                                                                                                    
    The Big Picture                                                                                   
                                                                                                      
                              EPOCH N STARTS                                               
                                   │                                                                  
         ┌─────────────────────────┼─────────────────────────┐                                        
         │                         ▼                         │                             
         │   ┌──────────────────────────────────────────┐    │                                        
         │   │         PHASE 1: CRANK SCORES            │    │                                        
         │   │                                          │    │                             
         │   │  Keeper sends ~54 txs, each with ~32     │    │                                        
         │   │  ValidatorHistory accounts as remaining  │    │                                        
         │   │  accounts. Program reads epoch_credits   │    │                             
         │   │  + commission, computes score, drops      │    │                                       
         │   │  into histogram bucket.                   │    │                                       
         │   └──────────────┬───────────────────────────┘    │                             
         │                  │ ~1,700 validators scored        │                                       
         │                  ▼                                 │                                       
         │   ┌──────────────────────────────────────────┐    │                             
         │   │       PHASE 2: COMPUTE THRESHOLD         │    │                                        
         │   │                                          │    │                                        
         │   │  Single tx. Walks histogram from top     │    │                             
         │   │  bucket (511) down, counting validators  │    │                                        
         │   │  until top 10% reached. That bucket's    │    │                                        
         │   │  lower bound = threshold.                │    │                             
         │   └──────────────┬───────────────────────────┘    │                                        
         │                  │ threshold stored on-chain       │                                       
         │                  ▼                                 │                            
         │   ┌──────────────────────────────────────────┐    │                                        
         │   │    PHASE 3: DELEGATE / UNDELEGATE         │    │                                       
         │   │                                          │    │                             
         │   │  Per-stake-account txs:                  │    │                                        
         │   │  • If stake is with a bad validator      │    │                                        
         │   │    → undelegate (deactivate)             │    │                             
         │   │  • If stake is inactive/cooled down      │    │                                        
         │   │    → delegate to a good validator        │    │                                        
         │   └──────────────────────────────────────────┘    │                             
         │                                                   │                                        
         └───────────────────────────────────────────────────┘                                        
                         REPEAT NEXT EPOCH                              
    Step-by-Step CLI Flow                                                                             
                                                                                                      
    0. One-Time Setup: Initialize                                                          
                                                                                                      
    # Build the program                                                                               
    quasar build                                                                           
                                                                                                      
    # Deploy to mainnet/devnet                                                                        
    quasar deploy                                                                          
                                                                                                      
    # Initialize the ScoringState PDA (only once ever)                                                
    # A keeper or anyone sends this tx:                                                    
    solana program invoke <PROGRAM_ID> \                                                              
      --data [0]  \                          # discriminator 0 = initialize                           
      --account <PAYER> writable signer \                                                  
      --account <SCORING_STATE_PDA> writable \                                                        
      --account 11111111111111111111111111111111  # system program                                    
                                                                                           
    This creates the ScoringState PDA on-chain:                                                       
    ┌─────────────────────────────────────────┐                                                       
    │ ScoringState PDA                        │                                            
    │ seeds: ["scoring_state"]                │                                                       
    │                                         │                                                       
    │ phase:         0 (IDLE)                 │                                            
    │ epoch:         0                        │                                                       
    │ threshold:     0                        │                                                       
    │ total_scored:  0                        │                                            
    │ histogram:     [0; 1024]  (512 buckets) │                                                       
    │ bitmap:        [0; 768]   (6144 bits)   │                                                       
    │ sa_bump:       <derived>                │                                            
    │ bump:          <derived>                │                                                       
    └─────────────────────────────────────────┘                                                       
                                                                                           
    ---                                                                                               
    1. Crank Scores (Keeper Bot Loop)                                                                 
                                                                                           
    The keeper detects a new epoch and starts cranking:                                               
                                                                                                      
    KEEPER BOT                           ON-CHAIN PROGRAM                                  
    ─────────                            ────────────────                                             
                                                                                                      
    Fetch all ValidatorHistory                                                             
    accounts via getProgramAccounts                                                                   
    (~1,700 accounts)                                                                                 
             │                                                                             
             ▼
    Split into batches of ~32                                                                           
    (limited by tx account limit)                                                                   
             │                                                                                        
             ├── Batch 1: validators 0-31 ──────►  crank_scores(disc=1)                               
             │                                      ├─ epoch changed? reset state          
             │                                      ├─ for each VH account:                           
             │                                      │   ├─ validate owner/disc                        
             │                                      │   ├─ check bitmap (dedup)            
             │                                      │   ├─ compute_score()                            
             │                                      │   ├─ bucket = score_to_bucket()                 
             │                                      │   ├─ histogram[bucket]++             
             │                                      │   └─ bitmap_set(index)                          
             │                                      └─ total_scored += batch_size                     
             │                                                                             
             ├── Batch 2: validators 32-63 ─────►  crank_scores(disc=1)                               
             │                                      └─ same logic, skips dupes                        
             ├── ...                                                                       
             └── Batch 54: validators 1696-1727 ─►  crank_scores(disc=1)                              
                                                                                                      
    Each crank_scores tx looks like:                                                       
                                                                                                      
    # Instruction data: [1]  (discriminator = 1)                                                      
    # Accounts:                                                                            
    #   0. payer (signer)                                                                             
    #   1. scoring_state (writable, PDA)                                                              
    #   --- remaining accounts ---                                                         
    #   2. validator_history_0 (readonly)                                                             
    #   3. validator_history_1 (readonly)                                                             
    #   ...                                                                                
    #   33. validator_history_31 (readonly)                                                           
                                                                                                      
    What happens inside the program for each validator:                                    
                                                                                                      
    ValidatorHistory Account (65,864 bytes)                                                           
    ┌──────────────────────────────────────┐                                               
    │ discriminator: [205,25,8,221,...]    │                                                          
    │ vote_account:  <pubkey>              │                                                          
    │ index:         42                    │◄── used for bitmap dedup                      
    │ ...                                  │                                                          
    │ history (circular buffer):           │                                                          
    │   idx: 4  (points to last entry)     │                                               
    │   ┌─────────────────────────────┐    │                                                          
    │   │ [0] epoch=6  credits=350k  │    │                                                           
    │   │     commission=5%          │    │                                                
    │   │ [1] epoch=7  credits=360k  │    │                                                           
    │   │     commission=5%          │    │                                                           
    │   │ [2] epoch=8  credits=340k  │    │                                                
    │   │     commission=10%         │    │                                                           
    │   │ [3] epoch=9  credits=370k  │    │                                                           
    │   │     commission=5%          │    │                                                
    │   │ [4] epoch=10 credits=355k  │◄── │  current
    │   │ [5] epoch=MAX (unset)      │    │                                                             
    │   │ ...                        │    │                                                         
    │   └─────────────────────────────┘    │                                                          
    └──────────────────────────────────────┘                                                          
                                                                                           
    Score calculation (last 5 epochs):                                                                
      epoch 6:  350,000 × (100-5)/100  = 332,500                                                      
      epoch 7:  360,000 × (100-5)/100  = 342,000                                           
      epoch 8:  340,000 × (100-10)/100 = 306,000                                                      
      epoch 9:  370,000 × (100-5)/100  = 351,500                                                      
      epoch 10: 355,000 × (100-5)/100  = 337,250                                           
                                                                                                      
      score = avg = (332,500 + 342,000 + 306,000 + 351,500 + 337,250) / 5                             
            = 333,850                                                                      
                                                                                                      
      bucket = 333,850 × 512 / 420,001 = 407                                                          
                                                                                           
      histogram[407] += 1                                                                             
                                                                                                      
    After all batches, the histogram looks something like:                                 
                                                                                                      
    Histogram (512 buckets, each ~820 score points)                                                   
    Count                                                                                  
      │                                                                                               
    50├──                                          ██                                                 
    40├──                                        ████                                      
    30├──                                      ████████                                               
    20├──                                   ████████████                                              
    10├──                              █████████████████████                               
      ├──────────────────██████████████████████████████████████                                       
      └────────────────────────────────────────────────────────                                       
      0                                                    511                             
      Score: 0                                        420,000                                         
                                               ▲                                                      
                                        90th percentile                                    
                                        threshold here                                                
                                                                                                      
    ---                                                                                    
    2. Compute Threshold (Single Tx)                                                                  
                                                                                                      
    # Instruction data: [2]  (discriminator = 2)                                           
    # Accounts:                                                                                       
    #   0. scoring_state (writable, PDA)                                                              
                                                                                           
    PROGRAM LOGIC:                                                                                    
                                                                                                      
      total_scored = 1,700                                                                 
      target_rank  = 1,700 / 10 = 170   (top 10% = 170 validators)                                    
                                                                                                      
      Walk from bucket 511 → 0:                                                            
        bucket 511: count=0,  running=0                                                               
        bucket 510: count=2,  running=2                                                                 
        bucket 509: count=5,  running=7                                                             
        ...                                                                                           
        bucket 408: count=12, running=165                                                             
        bucket 407: count=15, running=180  ← 180 >= 170, FOUND IT!                         
                                                                                                      
      threshold = 407 × 420,001 / 512 = 333,985                                                       
                                                                                           
      Store: scoring_state.threshold = 333,985                                                        
      Store: scoring_state.phase = THRESHOLD_COMPUTED (2)                                             
                                                                                           
    ---                                                                                               
    3a. Undelegate Bad Validators                                                                     
                                                                                           
    The keeper scans for stake accounts whose staker authority is the program's PDA, checks if they're
    delegated to underperforming validators:                                                          
                                                                                           
    KEEPER BOT                              ON-CHAIN PROGRAM                                          
    ─────────                               ────────────────                                          
                                                                                           
    For each managed stake account:                                                                   
      │                                                                                               
      ├─ Fetch stake account data                                                          
      ├─ Is it actively delegated?                                                                    
      │   └─ YES → fetch that validator's                                                             
      │             ValidatorHistory                                                       
      │             └─ recompute score                                                                
      │                 └─ score < threshold?                                                         
      │                     └─ YES → send undelegate tx                                    
      │                                                                                               
      └─ undelegate_stake(disc=4) ──────────►                                                         
                                              ├─ assert phase == THRESHOLD_COMPUTED        
                                              ├─ assert epoch is current                              
                                              ├─ read VH, compute score                               
                                              ├─ assert score < threshold ✓                
                                              ├─ assert stake is active ✓                             
                                              ├─ assert staker == our PDA ✓                           
                                              ├─ assert voter matches VH ✓                 
                                              └─ CPI → Stake::Deactivate                              
                                                   (signed by PDA)                                    
                                                                                           
    # Instruction data: [4]  (discriminator = 4)                                                      
    # Accounts:                                                                                       
    #   0. scoring_state (readonly, PDA)                                                   
    #   1. stake_account (writable)                                                                   
    #   2. validator_history (readonly)                                                               
    #   3. clock sysvar                                                                    
    #   4. stake_authority (PDA, seeds=["stake_authority"])                                           
    #   5. stake_program (Stake1111...)                                                               
                                                                                           
    The stake account starts cooling down (takes ~1 epoch).                                           
                         
    ---                                                                                             
    3b. Delegate to Good Validators                                                                   
                                                                                                      
    Once a stake account has fully deactivated (cooled down), or if it was never delegated:
                                                                                                      
    KEEPER BOT                              ON-CHAIN PROGRAM                                          
    ─────────                               ────────────────                               
                                                                                                      
    For each managed stake account:                                                                   
      │                                                                                    
      ├─ Is it inactive / fully deactivated?                                                          
      │   └─ YES → pick a good validator                                                              
      │             (score >= threshold)                                                   
      │             └─ send delegate tx                                                               
      │                                                                                               
      └─ delegate_stake(disc=3) ────────────►                                              
                                              ├─ assert phase == THRESHOLD_COMPUTED                   
                                              ├─ assert epoch is current                              
                                              ├─ read VH, compute score                    
                                              ├─ assert score >= threshold ✓                          
                                              ├─ assert stake is delegatable ✓                        
                                              ├─ assert staker == our PDA ✓                
                                              ├─ assert vote account matches ✓                        
                                              └─ CPI → Stake::DelegateStake                           
                                                   (signed by PDA)                         
                                                                                                      
    # Instruction data: [3]  (discriminator = 3)                                                      
    # Accounts:                                                                            
    #   0. scoring_state (readonly, PDA)                                                              
    #   1. stake_account (writable)                                                                   
    #   2. validator_history (readonly)                                                    
    #   3. validator_vote_account (readonly)                                                          
    #   4. clock sysvar                                                                               
    #   5. stake_history sysvar                                                            
    #   6. stake_config sysvar                                                                        
    #   7. stake_authority (PDA)                                                                      
    #   8. stake_program (Stake1111...)                                                    
                                                                                                      
    ---                                                                                               
    User Flow (How Stake Gets Into the System)                                             
                                                                                                      
    USER                        WEBSITE                      ON-CHAIN                                 
    ────                        ───────                      ────────                      
                                                                                                      
    Connects wallet ──────────► Lists user's                                                          
                                stake accounts                                             
                                     │                                                                
    Clicks "Opt In" ──────────► Builds tx calling                                                     
    on stake account            Stake program to                                           
                                transfer staker                              
                                authority to                                                            
                                Quanductor's PDA                                                    
                                     │                                                                
    Signs tx ─────────────────► ──────────────────────► Stake account                                 
                                                        staker authority                   
                                                        now = our PDA                                 
                                                             │                                        
                                KEEPER BOT detects ◄─────────┘                             
                                new managed stake                                                     
                                account and starts                                                    
                                managing it automatically                                  
                                                                                                      
    The Full Epoch Lifecycle                                                                          
                                                                                           
     Epoch N-1 ends, Epoch N begins                                                                   
             │                                                                                        
             ▼                                                                             
     ┌─── PHASE: IDLE ──────────────────────────────────────┐                                         
     │  First crank_scores tx auto-resets state:             │                                        
     │  • zeros histogram & bitmap                           │                             
     │  • sets epoch = N                                     │                                        
     │  • sets phase = CRANKING                              │                                        
     └──────────────────────────┬────────────────────────────┘                             
                                ▼                                                                     
     ┌─── PHASE: CRANKING ─────────────────────────────────┐                                          
     │  ~54 crank_scores txs over ~1 minute                 │                              
     │  (can be parallelized — bitmap prevents conflicts)   │                                         
     └──────────────────────────┬───────────────────────────┘                                         
                                ▼                                                          
     ┌─── compute_threshold (1 tx) ────────────────────────┐                                          
     │  phase → THRESHOLD_COMPUTED                          │                                         
     └──────────────────────────┬───────────────────────────┘                              
                                ▼                                                                     
     ┌─── PHASE: THRESHOLD_COMPUTED ───────────────────────┐                                          
     │  Keeper runs delegate/undelegate txs as needed       │                              
     │  Anyone can call these — fully permissionless        │                                         
     │  This phase lasts until next epoch resets everything │                                         
     └──────────────────────────────────────────────────────┘                              
                                │                                                                     
                         Epoch N+1 begins                                                             
                         (cycle repeats)                                                   
                                                                                                      
    Cost Per Epoch                                                                                    
                                                                                           
    Crank txs:     ~54 × 5,000 lamports  =  0.00027 SOL                                               
    Threshold tx:   1  × 5,000 lamports  =  0.000005 SOL                                              
    Delegate txs:   varies per stake acct                                                  
    ─────────────────────────────────────────────────────                                             
    Total:          < 0.02 SOL per epoch (~2 days)                                                    
                                                                                           
    Every instruction is permissionless — anyone can run the keeper bot, and multiple keepers can run 
    simultaneously without conflict (the bitmap prevents double-counting, and delegate/undelegate are 
    idempotent on state checks).                                                                     

  ↑/↓ to scroll · Space, Enter, or Escape to dismiss

