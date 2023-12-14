// These constants represent the RISC-V ELF and the image ID generated by risc0-build.
// The ELF is used for proving and the ID is used for verification.
use bitcoin::pow::CompactTarget;
use bridge_core::{calculate_double_sha256, parse_bytes_to_blockheader};
use bridge_methods::{GUEST_ELF, GUEST_ID};
use risc0_zkvm::{default_prover, ExecutorEnv};
use serde_json::Value;
use sha2::{Digest, Sha256};

// fn calculate_double_sha256(header: &[u8]) -> Vec<u8> {
//     let mut hasher = Sha256::new();
//     hasher.update(header);
//     let result = hasher.finalize_reset();
//     hasher.update(result);
//     hasher.finalize().to_vec()
// }

fn main() {
    // Initialize tracing. In order to view logs, run `RUST_LOG=info cargo run`
    env_logger::init();

    // An executor environment describes the configurations for the zkVM
    // including program inputs.
    // An default ExecutorEnv can be created like so:
    // `let env = ExecutorEnv::builder().build().unwrap();`
    // However, this `env` does not have any inputs.
    //
    // To add add guest input to the executor environment, use
    // ExecutorEnvBuilder::write().
    // To access this method, you'll need to use ExecutorEnv::builder(), which
    // creates an ExecutorEnvBuilder. When you're done adding input, call
    // ExecutorEnvBuilder::build().
    // This is the json data from https://mempool.space/api/v1/block/000000004ebadb55ee9096c9a2f8880e09da59c0d68b1c228da88e48844a1485
    let json_data = r#"{"id":"000000004ebadb55ee9096c9a2f8880e09da59c0d68b1c228da88e48844a1485","height":4,"version":1,"timestamp":1231470988,"bits":486604799,"nonce":2850094635,"difficulty":1,"merkle_root":"df2b060fa2e5e9c8ed5eaf6a45c13753ec8c63282b2688322eba40cd98ea067a","tx_count":1,"size":215,"weight":860,"previousblockhash":"0000000082b5015589a3fdf2d4baff403e6f0be035a5d9742c1cae6295464449","mediantime":1231469744,"extras":{"totalFees":0,"medianFee":0,"feeRange":[0,0,0,0,0,0,0],"reward":5000000000,"pool":{"id":0,"name":"Unknown","slug":"unknown"},"avgFee":0,"avgFeeRate":0,"coinbaseRaw":"04ffff001d011a","coinbaseAddress":null,"coinbaseSignature":"OP_PUSHBYTES_65 04184f32b212815c6e522e66686324030ff7e5bf08efb21f8b00614fb7690e19131dd31304c54f37baa40db231c918106bb9fd43373e37ae31a0befc6ecaefb867 OP_CHECKSIG","coinbaseSignatureAscii":"\u0004ÿÿ\u0000\u001d\u0001\u001a","avgTxSize":0,"totalInputs":0,"totalOutputs":1,"totalOutputAmt":0,"medianFeeAmt":0,"feePercentiles":[0,0,0,0,0,0,0],"segwitTotalTxs":0,"segwitTotalSize":0,"segwitTotalWeight":0,"header":"010000004944469562ae1c2c74d9a535e00b6f3e40ffbad4f2fda3895501b582000000007a06ea98cd40ba2e3288262b28638cec5337c1456aaf5eedc8e9e5a20f062bdf8cc16649ffff001d2bfee0a9","utxoSetChange":1,"utxoSetSize":4,"totalInputAmt":0,"virtualSize":215,"orphans":[],"matchRate":null,"expectedFees":null,"expectedWeight":null}}"#;

    // let v: Value = serde_json::from_str(json_data).unwrap();
    // let mut header = Vec::new();

    // let mut previous_block_hash = hex::decode(v["previousblockhash"].as_str().unwrap()).unwrap();
    // previous_block_hash.reverse();
    // let mut merkle_root = hex::decode(v["merkle_root"].as_str().unwrap()).unwrap();
    // merkle_root.reverse();
    // let nbits = CompactTarget::from_consensus(u32::try_from(v["bits"].as_u64().unwrap()).unwrap());
    // println!("nbits: {:?}", nbits);

    // header.extend(
    //     i32::try_from(v["version"].as_i64().unwrap())
    //         .unwrap()
    //         .to_le_bytes(),
    // );
    // header.extend(&previous_block_hash);
    // header.extend(&merkle_root);
    // header.extend(
    //     u32::try_from(v["timestamp"].as_u64().unwrap())
    //         .unwrap()
    //         .to_le_bytes(),
    // );
    // header.extend(
    //     u32::try_from(v["bits"].as_u64().unwrap())
    //         .unwrap()
    //         .to_le_bytes(),
    // );
    // header.extend(
    //     u32::try_from(v["nonce"].as_u64().unwrap())
    //         .unwrap()
    //         .to_le_bytes(),
    // );

    // let input: [u8; 80] = header.try_into().unwrap();

    let input: [u8; 80] = hex::decode("02000000b6ff0b1b1680a2862a30ca44d346d9e8910d334beb48ca0c00000000000000009d10aa52ee949386ca9385695f04ede270dda20810decd12bc9b048aaab3147124d95a5430c31b18fe9f0864").unwrap().try_into().unwrap();
    let block_header: bridge_core::BlockHeader = parse_bytes_to_blockheader(&input);

    println!("Input: {}", hex::encode(input));

    println!(
        "Double SHA256: {}",
        hex::encode(calculate_double_sha256(&input))
    );

    // For example:
    // let bitcoin_block_header = "02000000b6ff0b1b1680a2862a30ca44d346d9e8910d334beb48ca0c00000000000000009d10aa52ee949386ca9385695f04ede270dda20810decd12bc9b048aaab3147124d95a5430c31b18fe9f0864";
    // let input: [u8; 80] = hex::decode(bitcoin_block_header)
    //     .unwrap()
    //     .try_into()
    //     .unwrap();
    // let input1: [u8; 8] = input[0..8].try_into().unwrap();
    // let input2: [u8; 32] = input[8..40].try_into().unwrap();
    // let input3: [u8; 32] = input[40..72].try_into().unwrap();
    // let input4: [u8; 8] = input[72..80].try_into().unwrap();
    let env = ExecutorEnv::builder()
        .write(&block_header)
        .unwrap()
        // .write(&input2)
        // .unwrap()
        // .write(&input3)
        // .unwrap()
        // .write(&input4)
        // .unwrap()
        .build()
        .unwrap();

    // Obtain the default prover.
    let prover = default_prover();

    // Produce a receipt by proving the specified ELF binary.
    let receipt = prover.prove_elf(env, GUEST_ELF).unwrap();

    // TODO: Implement code for retrieving receipt journal here.

    // For example:
    let output: [u8; 32] = receipt.journal.decode().unwrap();

    println!("Output: {}", hex::encode(output));

    // Optional: Verify receipt to confirm that recipients will also be able to
    // verify your receipt
    receipt.verify(GUEST_ID).unwrap();
}
