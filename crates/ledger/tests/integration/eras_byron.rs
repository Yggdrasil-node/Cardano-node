use super::*;

/// Build a synthetic Byron EBB as CBOR bytes.
fn build_byron_ebb(epoch: u64, prev_hash: &[u8; 32]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(3);
    enc.array(5);
    enc.unsigned(764824073);
    enc.bytes(prev_hash);
    enc.bytes(&[0xAA; 32]);
    enc.array(2);
    enc.unsigned(epoch);
    enc.array(1).unsigned(0);
    enc.array(0);
    enc.array(0);
    enc.array(0);
    enc.into_bytes()
}

/// Build a synthetic Byron main block as CBOR bytes.
fn build_byron_main(epoch: u64, slot_in_epoch: u64, prev_hash: &[u8; 32]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(3);
    enc.array(5);
    enc.unsigned(764824073);
    enc.bytes(prev_hash);
    enc.bytes(&[0xBB; 32]);
    enc.array(4);
    enc.array(2);
    enc.unsigned(epoch);
    enc.unsigned(slot_in_epoch);
    enc.bytes(&[0xCC; 64]);
    enc.array(1).unsigned(1);
    enc.bytes(&[0xDD; 64]);
    enc.array(0);
    enc.array(0);
    enc.array(0);
    enc.into_bytes()
}

#[test]
fn byron_ebb_decode() {
    let prev_hash = [0x11; 32];
    let raw = build_byron_ebb(5, &prev_hash);
    let block = ByronBlock::decode_ebb(&raw).expect("decode EBB");
    assert_eq!(block.epoch(), 5);
    assert_eq!(*block.prev_hash(), prev_hash);
    assert_eq!(block.absolute_slot(BYRON_SLOTS_PER_EPOCH), 5 * 21600);
}

#[test]
fn byron_main_block_decode() {
    let prev_hash = [0x22; 32];
    let raw = build_byron_main(10, 500, &prev_hash);
    let block = ByronBlock::decode_main(&raw).expect("decode main block");
    assert_eq!(block.epoch(), 10);
    assert_eq!(*block.prev_hash(), prev_hash);
    assert_eq!(block.absolute_slot(BYRON_SLOTS_PER_EPOCH), 10 * 21600 + 500);
}

#[test]
fn byron_ebb_epoch_zero() {
    let raw = build_byron_ebb(0, &[0x00; 32]);
    let block = ByronBlock::decode_ebb(&raw).expect("decode EBB epoch 0");
    assert_eq!(block.epoch(), 0);
    assert_eq!(block.absolute_slot(BYRON_SLOTS_PER_EPOCH), 0);
}

#[test]
fn byron_main_block_first_slot() {
    let raw = build_byron_main(3, 0, &[0x33; 32]);
    let block = ByronBlock::decode_main(&raw).expect("decode first slot");
    assert_eq!(block.absolute_slot(BYRON_SLOTS_PER_EPOCH), 3 * 21600);
}

#[test]
fn byron_main_block_last_slot() {
    let raw = build_byron_main(7, 21599, &[0x44; 32]);
    let block = ByronBlock::decode_main(&raw).expect("decode last slot");
    assert_eq!(block.absolute_slot(BYRON_SLOTS_PER_EPOCH), 7 * 21600 + 21599);
}

#[test]
fn byron_block_variant_accessors() {
    let ebb = ByronBlock::EpochBoundary {
        epoch: 2,
        prev_hash: [0x55; 32],
        chain_difficulty: 0,
        raw_header: vec![],
    };
    assert_eq!(ebb.epoch(), 2);
    assert_eq!(*ebb.prev_hash(), [0x55; 32]);

    let main = ByronBlock::MainBlock {
        epoch: 3,
        slot_in_epoch: 100,
        prev_hash: [0x66; 32],
        chain_difficulty: 1,
        raw_header: vec![],
    };
    assert_eq!(main.epoch(), 3);
    assert_eq!(*main.prev_hash(), [0x66; 32]);
}