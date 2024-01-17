use std::borrow::BorrowMut;
use std::collections::HashMap;

use crate::actor::{Actor, EVMSignature};
use crate::merkle::MerkleTree;
use crate::user::User;
use crate::utils::{
    create_btc_tx, create_control_block, create_taproot_address, create_tx_ins, create_tx_outs,
    generate_n_of_n_script, generate_n_of_n_script_without_hash, handle_anyone_can_spend_script, create_kickoff_tx, handle_connector_binary_tree_script, generate_timelock_script, mine_blocks,
};
use crate::verifier::Verifier;
use bitcoin::address::NetworkChecked;
use bitcoin::consensus::serialize;
use bitcoin::psbt::Output;
use bitcoin::sighash::SighashCache;
use bitcoin::taproot::LeafVersion;
use bitcoin::transaction::Version;
use bitcoin::{absolute, hashes::Hash, secp256k1, secp256k1::schnorr, Address, Txid};
use bitcoin::{OutPoint, Transaction, TxOut, TxIn, ScriptBuf, Sequence, Witness, Amount};
use bitcoincore_rpc::{Client, RpcApi};
use circuit_helpers::config::BRIDGE_AMOUNT_SATS;
use circuit_helpers::constant::{EVMAddress, MIN_RELAY_FEE, HASH_FUNCTION_32};
use secp256k1::rand::Rng;
use secp256k1::rand::rngs::OsRng;
use secp256k1::schnorr::Signature;
use secp256k1::{All, Secp256k1, XOnlyPublicKey};
type PreimageType = [u8; 32];

pub fn check_deposit(
    secp: &Secp256k1<All>,
    rpc: &Client,
    utxo: OutPoint,
    hash: [u8; 32],
    return_address: XOnlyPublicKey,
    verifiers_pks: &Vec<XOnlyPublicKey>,
) -> absolute::Time {
    // 1. Check if txid is mined in bitcoin
    // 2. Check if 0th output of the txid has 1 BTC
    // 3. Check if 0th output of the txid's scriptpubkey is N-of-N multisig and Hash of preimage or return_address after 200 blocks
    // 4. If all checks pass, return true
    // 5. Return the blockheight of the block in which the txid was mined
    let tx_res = rpc
        .get_transaction(&utxo.txid, None)
        .unwrap_or_else(|e| panic!("Failed to get raw transaction: {}, txid: {}", e, utxo.txid));
    let tx = tx_res.transaction().unwrap();
    // println!("tx: {:?}", tx);
    println!("txid: {:?}", tx.txid());
    println!("utxo: {:?}", utxo);
    assert!(tx.output[utxo.vout as usize].value == bitcoin::Amount::from_sat(BRIDGE_AMOUNT_SATS));
    let (address, _) = User::generate_deposit_address(secp, verifiers_pks, hash, return_address);
    // println!("address: {:?}", address);
    // println!("address.script_pubkey(): {:?}", address.script_pubkey());
    assert!(tx.output[utxo.vout as usize].script_pubkey == address.script_pubkey());
    let time = tx_res.info.blocktime.unwrap() as u32;
    println!("time: {:?}", time);
    return absolute::Time::from_consensus(time).unwrap();
}

pub fn create_connector_tree_preimages(depth: u32, rng: &mut OsRng) -> Vec<Vec<PreimageType>> {
    let mut connector_tree_preimages: Vec<Vec<PreimageType>> = Vec::new();
    let root_preimage: PreimageType = rng.gen();
    connector_tree_preimages.push(vec![root_preimage]);
    for i in 1..(depth + 1) {
        let mut current_level: Vec<PreimageType> = Vec::new();
        for _ in 0..2u32.pow(i) {
            current_level.push(rng.gen());
        }
        connector_tree_preimages.push(current_level);
    }
    connector_tree_preimages
}

#[derive(Debug, Clone)]
pub struct DepositPresigns {
    pub rollup_sign: EVMSignature,
    pub kickoff_sign: schnorr::Signature,
    pub move_bridge_sign_utxo_pairs: HashMap<OutPoint, schnorr::Signature>,
    pub operator_take_signs: Vec<schnorr::Signature>,
}

#[derive(Debug, Clone)]
pub struct Operator<'a> {
    pub rpc: &'a Client,
    pub signer: Actor,
    pub verifiers_pks: Vec<XOnlyPublicKey>,
    pub verifier_evm_addresses: Vec<EVMAddress>,
    pub deposit_presigns: HashMap<Txid, Vec<DepositPresigns>>,
    pub deposit_merkle_tree: MerkleTree,
    pub withdrawals_merkle_tree: MerkleTree,
    pub withdrawals_payment_txids: Vec<Txid>,
    pub mock_verifier_access: Vec<Verifier<'a>>, // on production this will be removed rather we will call the verifier's API
    pub preimages: Vec<PreimageType>,
    connector_tree_preimages: Vec<Vec<PreimageType>>,

}

pub fn check_presigns(
    utxo: OutPoint,
    timestamp: absolute::Time,
    deposit_presigns: &DepositPresigns,
) {
}

impl<'a> Operator<'a> {
    pub fn new(rng: &mut OsRng, rpc: &'a Client) -> Self {
        let signer = Actor::new(rng);
        let connector_tree_preimages = create_connector_tree_preimages(3, rng);
        Self {
            rpc,
            signer,
            verifiers_pks: Vec::new(),
            verifier_evm_addresses: Vec::new(),
            deposit_presigns: HashMap::new(),
            deposit_merkle_tree: MerkleTree::initial(),
            withdrawals_merkle_tree: MerkleTree::initial(),
            withdrawals_payment_txids: Vec::new(),
            mock_verifier_access: Vec::new(),
            preimages: Vec::new(),
            connector_tree_preimages: connector_tree_preimages,
        }
    }

    pub fn add_verifier(&mut self, verifier: &Verifier<'a>) {
        self
            .mock_verifier_access
            .push(verifier.clone());
        self.verifiers_pks.push(verifier.signer.xonly_public_key.clone());
        self.verifier_evm_addresses.push(verifier.signer.evm_address.clone());
    }

    pub fn get_all_verifiers(&self) -> Vec<XOnlyPublicKey> {
        let mut all_verifiers = self.verifiers_pks.to_vec();
        all_verifiers.push(self.signer.xonly_public_key.clone());
        all_verifiers
    }
    // this is a public endpoint that every depositor can call
    pub fn new_deposit(
        &mut self,
        utxo: OutPoint,
        hash: [u8; 32],
        return_address: XOnlyPublicKey,
        evm_address: EVMAddress,
    ) -> Vec<EVMSignature> {
        // self.verifiers + signer.public_key
        let all_verifiers = self.get_all_verifiers();
        // println!("all_verifiers checking: {:?}", all_verifiers);
        let timestamp = check_deposit(
            &self.signer.secp,
            self.rpc,
            utxo,
            hash,
            return_address.clone(),
            &all_verifiers,
        );
        println!("mock verifier access: {:?}", self.mock_verifier_access);
        let presigns_from_all_verifiers = self
            .mock_verifier_access
            .iter()
            .map(|verifier| {
                println!("verifier in the closure: {:?}", verifier);
                // Note: In this part we will need to call the verifier's API to get the presigns
                let deposit_presigns =
                    verifier.new_deposit(utxo, hash, return_address.clone(), evm_address, &all_verifiers);
                    println!("checked new deposit");
                check_presigns(utxo, timestamp, &deposit_presigns);
                println!("checked presigns");
                deposit_presigns
            })
            .collect::<Vec<_>>();
        println!("presigns_from_all_verifiers: done");

        let (anyone_can_spend_script_pub_key, dust_value) = handle_anyone_can_spend_script();

        let kickoff_tx = create_kickoff_tx(vec![utxo], vec![
            (
                bitcoin::Amount::from_sat(BRIDGE_AMOUNT_SATS)
                    - dust_value
                    - bitcoin::Amount::from_sat(MIN_RELAY_FEE),
                generate_n_of_n_script_without_hash(&all_verifiers),
            ),
            (dust_value, anyone_can_spend_script_pub_key),
        ]);

        let kickoff_txid = kickoff_tx.txid();

        let rollup_sign = self.signer.sign_deposit(
            kickoff_txid,
            evm_address,
            hash,
            timestamp.to_consensus_u32().to_be_bytes(),
        );
        let mut all_rollup_signs = presigns_from_all_verifiers
            .iter()
            .map(|presigns| presigns.rollup_sign)
            .collect::<Vec<_>>();
        all_rollup_signs.push(rollup_sign);
        self.deposit_presigns
            .insert(utxo.txid, presigns_from_all_verifiers);
        all_rollup_signs
    }

    // this is called when a Withdrawal event emitted on rollup
    pub fn new_withdrawal(&mut self, withdrawal_address: Address<NetworkChecked>) {
        let taproot_script = withdrawal_address.script_pubkey();
        // we are assuming that the withdrawal_address is a taproot address so we get the last 32 bytes
        let hash: [u8; 34] = taproot_script.as_bytes().try_into().unwrap();
        let hash: [u8; 32] = hash[2..].try_into().unwrap();

        // 1. Add the address to WithdrawalsMerkleTree
        self.withdrawals_merkle_tree.add(hash);

        // self.withdrawals_merkle_tree.add(withdrawal_address.to);

        // 2. Pay to the address and save the txid
        let txid = self
            .rpc
            .send_to_address(
                &withdrawal_address,
                bitcoin::Amount::from_sat(1),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        self.withdrawals_payment_txids.push(txid);
    }

    // this is called when a Deposit event emitted on rollup
    pub fn preimage_revealed(
        &mut self,
        preimage: PreimageType,
        utxo: OutPoint,
        return_address: XOnlyPublicKey, // TODO: SAVE THIS TO STRUCT
    ) -> OutPoint {
        self.preimages.push(preimage);
        // 1. Add the corresponding txid to DepositsMerkleTree
        self.deposit_merkle_tree.add(utxo.txid.to_byte_array());
        let hash = HASH_FUNCTION_32(preimage);
        let all_verifiers = self.get_all_verifiers();
        let script_n_of_n = generate_n_of_n_script(&all_verifiers, hash);

        let script_n_of_n_without_hash = generate_n_of_n_script_without_hash(&all_verifiers);
        let (address, _) =
            create_taproot_address(&self.signer.secp, vec![script_n_of_n_without_hash.clone()]);

        let (anyone_can_spend_script_pub_key, dust_value) = handle_anyone_can_spend_script();

        let mut kickoff_tx = create_kickoff_tx(vec![utxo], vec![
            (
                bitcoin::Amount::from_sat(BRIDGE_AMOUNT_SATS)
                    - dust_value
                    - bitcoin::Amount::from_sat(MIN_RELAY_FEE),
                address.script_pubkey(),
            ),
            (dust_value, anyone_can_spend_script_pub_key),
        ]);

        let (deposit_address, deposit_taproot_info) =
            User::generate_deposit_address(&self.signer.secp, &all_verifiers, hash, return_address);

        let prevouts = create_tx_outs(vec![(
            bitcoin::Amount::from_sat(BRIDGE_AMOUNT_SATS),
            deposit_address.script_pubkey(),
        )]);

        let mut kickoff_signatures: Vec<Signature> = Vec::new();
        let deposit_presigns_for_kickoff = self
            .deposit_presigns
            .get(&utxo.txid)
            .expect("Deposit presigns not found");
        for presign in deposit_presigns_for_kickoff.iter() {
            kickoff_signatures.push(presign.kickoff_sign);
        }

        let sig =
            self.signer
                .sign_taproot_script_spend_tx(&mut kickoff_tx, prevouts, &script_n_of_n, 0);
        kickoff_signatures.push(sig);

        let spend_control_block = deposit_taproot_info
            .control_block(&(script_n_of_n.clone(), LeafVersion::TapScript))
            .expect("Cannot create control block");

        let mut sighash_cache = SighashCache::new(kickoff_tx.borrow_mut());
        let witness = sighash_cache.witness_mut(0).unwrap();
        // push signatures to witness
        witness.push(preimage);
        kickoff_signatures.reverse();
        for sig in kickoff_signatures.iter() {
            witness.push(sig.as_ref());
        }

        witness.push(script_n_of_n);
        witness.push(&spend_control_block.serialize());
        // println!("witness size: {:?}", witness.size());
        println!("kickoff_tx: {:?}", kickoff_tx);
        let kickoff_txid = kickoff_tx.txid();
        // println!("kickoff_txid: {:?}", kickoff_txid);
        let utxo_for_child = OutPoint {
            txid: kickoff_txid,
            vout: 1,
        };

        let child_tx = self.create_child_pays_for_parent(utxo_for_child);
        let rpc_kickoff_txid = self.rpc.send_raw_transaction(&kickoff_tx).unwrap();
        println!("rpc_kickoff_txid: {:?}", rpc_kickoff_txid);
        let child_of_kickoff_txid = self.rpc.send_raw_transaction(&child_tx).unwrap();
        println!("child_of_kickoff_txid: {:?}", child_of_kickoff_txid);
        OutPoint {
            txid: kickoff_txid,
            vout: 0,
        }
    }

    pub fn create_child_pays_for_parent(&self, parent_outpoint: OutPoint) -> Transaction {
        let resource_tx_id = self
            .rpc
            .send_to_address(
                &self.signer.address,
                bitcoin::Amount::from_sat(BRIDGE_AMOUNT_SATS),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let resource_tx = self.rpc.get_raw_transaction(&resource_tx_id, None).unwrap();
        println!("resource_tx: {:?}", resource_tx);
        let vout = resource_tx
            .output
            .iter()
            .position(|x| x.value == bitcoin::Amount::from_sat(BRIDGE_AMOUNT_SATS))
            .unwrap();

        let all_verifiers = self.get_all_verifiers();

        let script_n_of_n_without_hash = generate_n_of_n_script_without_hash(&all_verifiers);
        let (address, _) =
            create_taproot_address(&self.signer.secp, vec![script_n_of_n_without_hash.clone()]);

        let (anyone_can_spend_script_pub_key, dust_value) = handle_anyone_can_spend_script();

        let child_tx_ins = create_tx_ins(vec![
            parent_outpoint,
            OutPoint {
                txid: resource_tx_id,
                vout: vout as u32,
            },
        ]);

        let child_tx_outs = create_tx_outs(vec![
            (
                bitcoin::Amount::from_sat(BRIDGE_AMOUNT_SATS)
                    - dust_value
                    - bitcoin::Amount::from_sat(MIN_RELAY_FEE),
                address.script_pubkey(),
            ),
            (dust_value, anyone_can_spend_script_pub_key.clone()),
        ]);

        let mut child_tx = create_btc_tx(child_tx_ins, child_tx_outs);

        child_tx.input[0].witness.push([0x51]);

        let prevouts = create_tx_outs(vec![
            (dust_value, anyone_can_spend_script_pub_key),
            (
                bitcoin::Amount::from_sat(BRIDGE_AMOUNT_SATS),
                self.signer.address.script_pubkey(),
            ),
        ]);
        let sig = self
            .signer
            .sign_taproot_pubkey_spend_tx(&mut child_tx, prevouts, 1);
        let mut sighash_cache = SighashCache::new(child_tx.borrow_mut());
        let witness = sighash_cache.witness_mut(1).unwrap();
        witness.push(sig.as_ref());
        println!("child_tx: {:?}", child_tx);
        println!("child_txid: {:?}", child_tx.txid());
        child_tx
    }

    // this function is interal, where it checks if the current bitcoin height reaced to th end of the period,
    pub fn period1_end(&self) {
        // self.move_bridge_funds();

        // Check if all deposists are satisifed, all remaning bridge funds are moved to a new multisig
    }

    // this function is interal, where it checks if the current bitcoin height reaced to th end of the period,
    pub fn period2_end(&self) {
        // This is the time we generate proof.
    }

    // this function is interal, where it checks if the current bitcoin height reaced to th end of the period,
    pub fn period3_end(&self) {
        // This is the time send generated proof along with k-deep proof
        // and revealing bit-commitments for the next bitVM instance.
    }

    // this function is interal, where it moves remaining bridge funds to a new multisig using DepositPresigns
    pub fn move_single_bridge_fund(&self, deposit_txid: Txid, prev_outpoint: OutPoint) -> OutPoint {
        // 1. Get the deposit tx
        let prev_tx = self
            .rpc
            .get_raw_transaction(&prev_outpoint.txid, None)
            .unwrap();
        let utxo_amount = prev_tx.output[prev_outpoint.vout as usize].value;
        let all_verifiers = self.get_all_verifiers();

        let script_n_of_n_without_hash = generate_n_of_n_script_without_hash(&all_verifiers);
        let (address, tree_info) =
            create_taproot_address(&self.signer.secp, vec![script_n_of_n_without_hash.clone()]);

        let (anyone_can_spend_script_pub_key, dust_value) = handle_anyone_can_spend_script();

        let move_tx_ins = create_tx_ins(vec![prev_outpoint]);

        let move_tx_outs = create_tx_outs(vec![
            (
                utxo_amount - dust_value - bitcoin::Amount::from_sat(MIN_RELAY_FEE),
                address.script_pubkey(),
            ),
            (dust_value, anyone_can_spend_script_pub_key),
        ]);

        let mut move_tx = create_btc_tx(move_tx_ins, move_tx_outs);

        let mut move_signatures: Vec<Signature> = Vec::new();
        let deposit_presigns_from_txid = self
            .deposit_presigns
            .get(&deposit_txid)
            .expect("Deposit presigns not found");
        for presign in deposit_presigns_from_txid.iter() {
            move_signatures.push(
                presign
                    .move_bridge_sign_utxo_pairs
                    .get(&prev_outpoint)
                    .expect("No signatures for such utxo")
                    .clone(),
            );
        }

        let prevouts = vec![TxOut {
            script_pubkey: address.script_pubkey(),
            value: utxo_amount,
        }];

        let sig = self.signer.sign_taproot_script_spend_tx(
            &mut move_tx,
            prevouts,
            &script_n_of_n_without_hash,
            0,
        );
        move_signatures.push(sig);

        let spend_control_block = create_control_block(tree_info, &script_n_of_n_without_hash);

        let mut sighash_cache = SighashCache::new(move_tx.borrow_mut());
        let witness = sighash_cache.witness_mut(0).unwrap();
        // push signatures to witness
        move_signatures.reverse();
        for sig in move_signatures.iter() {
            witness.push(sig.as_ref());
        }

        witness.push(script_n_of_n_without_hash);
        witness.push(&spend_control_block.serialize());
        println!("move_tx: {:?}", move_tx);
        let move_txid = self.rpc.send_raw_transaction(&move_tx).unwrap();
        println!("move_txid: {:?}", move_txid);
        let move_tx_from_rpc = self.rpc.get_raw_transaction(&move_txid, None).unwrap();
        println!("move_tx_from_rpc: {:?}", move_tx_from_rpc);
        OutPoint {
            txid: move_txid,
            vout: 0,
        }
    }

    // This function is internal, it gives the appropriate response for a bitvm challenge
    pub fn challenge_received() {}

    pub fn spend_connector_tree_utxo(&self, utxo: OutPoint, tx: &mut Transaction, preimage: PreimageType) {
        let hash = HASH_FUNCTION_32(preimage);
        let (_, pubkey, address, tree_info) =
        handle_connector_binary_tree_script(
            &self.signer.secp,
            self.signer.xonly_public_key,
            1, // MAKE THIS CONFIGURABLE
            hash,
        );

        let utxo_tx = self.rpc.get_raw_transaction(&utxo.txid, None).unwrap();
        let timelock_script = generate_timelock_script(self.signer.xonly_public_key, 1);

        let sig = self.signer.sign_taproot_script_spend_tx(
            tx,
            vec![utxo_tx.output[utxo.vout as usize].clone()],
            &timelock_script,
            0,
        );
        let spend_control_block = tree_info
            .control_block(&(timelock_script.clone(), LeafVersion::TapScript))
            .expect("Cannot create control block");
        let mut sighash_cache = SighashCache::new(tx.borrow_mut());
        let witness = sighash_cache.witness_mut(0).unwrap();
        witness.push(sig.as_ref());
        witness.push(timelock_script);
        witness.push(&spend_control_block.serialize());
        let bytes_tx = serialize(&tx);
        // println!("bytes_connector_tree_tx length: {:?}", bytes_connector_tree_tx.len());
        // let hex_utxo_tx = hex::encode(bytes_utxo_tx.clone());
        let spending_txid = self
            .rpc
            .send_raw_transaction(&bytes_tx)
            .unwrap();
        println!("spending_txid: {:?}", spending_txid);
    }

}

pub fn create_connector_tree_tx(utxo: &OutPoint, depth: u32, first_address: Address, second_address: Address, dust_value: Amount, fee: Amount) -> Transaction {
    // UTXO value should be at least 2^depth * dust_value + (2^depth-1) * fee
    Transaction {
        version: Version(2),
        lock_time: absolute::LockTime::from_consensus(0),
        input: vec![TxIn {
            previous_output: *utxo,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::from_height(2),
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: (dust_value * 2u64.pow(depth)) + (fee * (2u64.pow(depth) - 1)),
            script_pubkey: first_address.script_pubkey(),
        },
        TxOut {
            value: (dust_value * 2u64.pow(depth)) + (fee * (2u64.pow(depth) - 1)),
            script_pubkey: second_address.script_pubkey(),
        }],
    }
}

    // This function creates the connector binary tree for operator to be able to claim the funds that they paid out of their pocket.
    // Depth will be determined later.
    pub fn create_connector_binary_tree(rpc: &Client, signer: &Actor, depth: u32, resource_utxo: OutPoint, dust_value: Amount, fee: Amount, connector_tree_preimages: Vec<Vec<PreimageType>>) -> (Vec<Vec<OutPoint>>, Vec<Vec<Transaction>>) {
        // UTXO value should be at least 2^depth * dust_value + (2^depth-1) * fee
        let total_amount = (dust_value * 2u64.pow(depth)) + (fee * (2u64.pow(depth) - 1));
        println!("total_amount: {:?}", total_amount);

        println!("connnector_tree_preimages: {:?}", connector_tree_preimages);

        let connector_tree_hashes = connector_tree_preimages.iter().map(|preimage_vecs| {
            preimage_vecs.iter().map(|preimage| HASH_FUNCTION_32(*preimage)).collect::<Vec<_>>()
        }).collect::<Vec<_>>();
        println!("connector_tree_hashes: {:?}", connector_tree_hashes);

        let (root_amount, root_pubkey, root_address, root_tree_info) =
                handle_connector_binary_tree_script(
                    &signer.secp,
                    signer.xonly_public_key,
                    1, // MAKE THIS CONFIGURABLE
                    connector_tree_hashes[0][0],
                );
        println!("root dust value: {:?}", root_address.clone().script_pubkey().dust_value());
        

        // let mut create_root_tx = Transaction {
        //     version: Version(2),
        //     lock_time: absolute::LockTime::from_consensus(0),
        //     input: vec![TxIn {
        //         previous_output: resource_utxo,
        //         script_sig: ScriptBuf::new(),
        //         sequence: bitcoin::transaction::Sequence::ENABLE_RBF_NO_LOCKTIME,
        //         witness: Witness::new(),
        //     }],
        //     output: vec![TxOut {
        //         value: total_amount,
        //         script_pubkey: root_address.script_pubkey(),
        //     }],
        // };
        // println!("create_root_tx: {:?}", create_root_tx);

        // let root_txid = create_root_tx.txid();
        // println!("root_txid: {:?}", root_txid);

        let rpc_txid = rpc.send_to_address(&root_address, total_amount, None, None, None, None, None, None).unwrap();
        println!("rpc_txid: {:?}", rpc_txid);

        mine_blocks(rpc, 3);

        let vout = rpc.get_raw_transaction(&rpc_txid, None).unwrap().output.iter().position(|x| x.value == total_amount).unwrap();

        let mut utxo_binary_tree: Vec<Vec<OutPoint>> = Vec::new();
        let mut tx_binary_tree: Vec<Vec<Transaction>> = Vec::new();
        let root_utxo = OutPoint {
            txid: rpc_txid,
            vout: vout as u32,
        };
        utxo_binary_tree.push(vec![root_utxo.clone()]);

        for i in 0..depth {
            let mut utxo_tree_current_level: Vec<OutPoint> = Vec::new();
            let utxo_tree_previous_level = utxo_binary_tree.last().unwrap();

            let mut tx_tree_current_level: Vec<Transaction> = Vec::new();

            for (j, utxo) in utxo_tree_previous_level.iter().enumerate() {
                let (_, _, first_address, _) =
                handle_connector_binary_tree_script(
                    &signer.secp,
                    signer.xonly_public_key,
                    1, // MAKE THIS CONFIGURABLE
                    connector_tree_hashes[(i + 1) as usize][2 * j],
                );
                let (_, _, second_address, _) =
                handle_connector_binary_tree_script(
                    &signer.secp,
                    signer.xonly_public_key,
                    1, // MAKE THIS CONFIGURABLE
                    connector_tree_hashes[(i + 1) as usize][2 * j + 1],
                );

                let tx = create_connector_tree_tx(utxo, depth - i - 1, first_address.clone(), second_address.clone(), dust_value, fee);
                let txid = tx.txid();
                let first_utxo = OutPoint {
                    txid,
                    vout: 0,
                };
                let second_utxo = OutPoint {
                    txid,
                    vout: 1,
                };
                utxo_tree_current_level.push(first_utxo);
                utxo_tree_current_level.push(second_utxo);
                tx_tree_current_level.push(tx);
            }
            utxo_binary_tree.push(utxo_tree_current_level);
            tx_binary_tree.push(tx_tree_current_level);
        }

        println!("utxo_binary_tree: {:?}", utxo_binary_tree);
        println!("tx_binary_tree: {:?}", tx_binary_tree);

        (utxo_binary_tree, tx_binary_tree)


    }

#[cfg(test)]
mod tests {
    use std::borrow::BorrowMut;

    use bitcoin::OutPoint;
    use bitcoincore_rpc::{Client, Auth, RpcApi};
    use secp256k1::rand::rngs::OsRng;

    use crate::{operator::{Operator, create_connector_binary_tree}, user::User, utils::mine_blocks};



    #[test]
    fn test_connector_tree_tx() {
        let rpc = Client::new(
            "http://localhost:18443/wallet/admin",
            Auth::UserPass("admin".to_string(), "admin".to_string()),
        )
        .unwrap_or_else(|e| panic!("Failed to connect to Bitcoin RPC: {}", e));
        let operator = Operator::new(&mut OsRng, &rpc);
        let resource_tx_id = operator
            .rpc
            .send_to_address(
                &operator.signer.address,
                bitcoin::Amount::from_sat(100_000_000),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let resource_tx = operator
            .rpc
            .get_raw_transaction(&resource_tx_id, None)
            .unwrap();
        println!("resource_tx: {:?}", resource_tx);

        let resource_txid = resource_tx.txid();
        let vout = resource_tx
            .output
            .iter()
            .position(|x| x.value == bitcoin::Amount::from_sat(100_000_000))
            .unwrap();
    
        let resource_utxo = OutPoint {
            txid: resource_txid,
            vout: vout as u32,
        };
        println!("resource_utxo: {:?}", resource_utxo);

        let (utxo_tree, tx_tree) = create_connector_binary_tree(&rpc, &operator.signer, 3, resource_utxo, bitcoin::Amount::from_sat(1000), bitcoin::Amount::from_sat(300), operator.connector_tree_preimages.clone());

        mine_blocks(&rpc, 3);

        for (i, utxo_level) in utxo_tree[0..utxo_tree.len() - 1].iter().enumerate() {
            for (j, utxo) in utxo_level.iter().enumerate() {
                let mut tx = tx_tree[i][j].clone();
                let preimage = operator.connector_tree_preimages[i][j];
                println!("first tx to start spending the tree: {:?}", tx);
                println!("first txid to start spending the tree: {:?}", tx.txid());
                println!("preimage: {:?}", preimage);
                operator.spend_connector_tree_utxo(*utxo, tx.borrow_mut(), preimage);
                let txid = tx.txid();
                let tx_from_rpc = operator.rpc.get_raw_transaction(&txid, None).unwrap();
                println!("tx_from_rpc: {:?}", tx_from_rpc);
                assert!(tx_from_rpc.output[0].value == tx.output[0].value);
                assert!(tx_from_rpc.output[1].value == tx.output[1].value);
            }
            mine_blocks(&rpc, 3);
        }

    }   

}