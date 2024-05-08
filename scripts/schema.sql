begin;

create table new_deposit_requests (
    start_utxo text,
    return_address text,
    evm_address text
);

create table deposit_move_txs (
    id serial primary key,
    move_txid text not null unique check (move_txid ~ '^[a-fA-F0-9]{64}')
);

commit;
