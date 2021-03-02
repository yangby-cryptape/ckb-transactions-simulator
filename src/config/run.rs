use std::{collections::HashMap, thread, time, vec::IntoIter};

use ckb_jsonrpc_types as rpc;
use ckb_types::{bytes, core, packed, prelude::*, H256};

use crate::{
    client::Client,
    error::{Error, Result},
    storage::Storage,
    types::{BlockMeta, CellInfo, InputInfo, LockGenerator, LockInfo, LockScriptId},
};

const BYTE_SHANNONS: u64 = 100_000_000;

impl super::RunConfig {
    pub(super) fn execute(&self) -> Result<()> {
        log::info!("Run ...");

        let stg = &self.storage;
        let cli = &self.client;
        let cfg = &self.config;

        let metadata = stg.get_metadata()?;
        let accounts = metadata.accounts()?;
        let lock_deps_dict = metadata.lock_deps_dict();
        let input_size_generator = cfg.generator.input_size_generator()?;
        let lock_generator = cfg.generator.lock_generator(&accounts)?;

        log::info!("checking the chain ...");
        cli.check_chain(&metadata.start_block)?;

        loop {
            log::info!("synchroning the blocks ...");
            let skip_sync = synchronize(
                &cli,
                &stg,
                &accounts,
                metadata.start_block.number,
                cfg.delay_blocks,
            )?;

            log::debug!("sending transactions ...");
            {
                let mut cells_iter = stg.load_cells()?.into_iter();
                let mut total_inputs = Vec::new();
                let mut loop_counter = 0;
                let mut expected_input_size = 0;
                loop {
                    if expected_input_size == 0 {
                        expected_input_size = input_size_generator.generate();
                    }
                    log::trace!(
                        "try fetch inputs {} -> {}",
                        total_inputs.len(),
                        expected_input_size
                    );
                    match fetch_more_inputs(
                        &mut cells_iter,
                        &mut total_inputs,
                        cfg.generator.inputs_limit,
                        cfg.generator.output_min_capacity,
                        expected_input_size,
                    )? {
                        FetchInputsResult::Lack => break,
                        FetchInputsResult::Next => continue,
                        FetchInputsResult::Enough => {
                            expected_input_size = 0;
                        }
                    }
                    loop_counter += 1;
                    log::trace!("selected {} inputs", total_inputs.len());

                    let (lock_hashes, inputs) = prepare_inputs(&mut total_inputs);

                    let rtx = construct_raw_transaction(
                        &inputs,
                        &accounts,
                        &lock_generator,
                        &lock_deps_dict,
                        cfg.generator.outputs_limit,
                        cfg.generator.output_capacity,
                        cfg.generator.tx_fee,
                    )?;
                    let stx = sign_transaction(rtx, &lock_hashes, &accounts)?;
                    let tx_hash = stx.calc_tx_hash();
                    let stx_json: rpc::Transaction = stx.into();
                    match cli.send_transaction(stx_json.clone()) {
                        Ok(_) => {
                            log::debug!("send tx {:#x} is ok", tx_hash);
                            sleep_millis(cfg.client.success_interval);
                            for input in inputs {
                                stg.spend_cell(input.out_point)?;
                            }
                        }
                        Err(err) => {
                            log::error!("send tx {:#x} failed since: {}", tx_hash, err);
                            let stx_str =
                                serde_json::to_string_pretty(&stx_json).map_err(Error::runtime)?;
                            log::debug!("tx {:#x} = {}", tx_hash, stx_str);
                            sleep_millis(cfg.client.failure_interval);
                            break;
                        }
                    }
                }
                if skip_sync && loop_counter == 0 {
                    log::trace!(
                        "waiting {} ms for new blocks and unspent cells ...",
                        cfg.client.idle_interval
                    );
                    sleep_millis(cfg.client.idle_interval);
                }
            }
        }
    }
}

fn sleep_millis(interval: u64) {
    thread::sleep(time::Duration::from_millis(interval));
}

impl Client {
    fn check_chain(&self, start_meta: &BlockMeta) -> Result<()> {
        let start_header = self
            .get_header_by_number(start_meta.number)?
            .ok_or_else(|| Error::runtime("the provided node doesn't have enough chain data"))?;
        if start_header.hash != start_meta.hash {
            let errmsg = format!(
                "the provided node isn't the expected chain, block#{}'s hash should be {:#x}, but got {:#x}",
                start_meta.number, start_meta.hash, start_header.hash
            );
            Err(Error::runtime(errmsg))
        } else {
            Ok(())
        }
    }
}

fn synchronize(
    cli: &Client,
    stg: &Storage,
    accounts: &HashMap<H256, LockInfo>,
    start_block: core::BlockNumber,
    delay_blocks: core::BlockNumber,
) -> Result<bool> {
    let next_num = stg.get_next_number()?.unwrap_or(start_block);
    let tip_num = cli.get_tip_block_number()?;
    let search_when_num = next_num + delay_blocks;
    log::trace!(
        "current tip: {}, next to search: {}, search when {}",
        tip_num,
        next_num,
        search_when_num
    );
    let skip_sync = next_num + delay_blocks >= tip_num;
    if !skip_sync {
        log::debug!("synchronizing to block#{} ...", tip_num - delay_blocks);
        for num in next_num..=(tip_num - delay_blocks) {
            log::trace!("fetching block#{} ...", num);
            let block = cli.get_block_by_number(num)?.ok_or_else(|| {
                let errmsg = format!("block#{} should exists but CKB node returns None", num);
                Error::runtime(errmsg)
            })?;
            for tx in &block.transactions {
                for (index, output_json) in tx.inner.outputs.iter().enumerate() {
                    let output: packed::CellOutput = output_json.clone().into();
                    for (hash, lock_info) in accounts {
                        if output.lock() == lock_info.script {
                            log::trace!("found a new cell {:#x}.{}", tx.hash, index);
                            let out_point = packed::OutPoint::new_builder()
                                .tx_hash(tx.hash.pack())
                                .index(index.pack())
                                .build();
                            let output_cap = output.capacity();
                            let cell_info = CellInfo::new(output_cap.unpack(), hash.clone());
                            stg.add_cell(out_point, cell_info)?;
                        }
                    }
                }
                for input in &tx.inner.inputs {
                    let out_point: packed::OutPoint = input.previous_output.clone().into();
                    stg.rm_cell(out_point)?;
                }
            }
            stg.put_prev_number(num)?;
        }
    };
    Ok(skip_sync)
}

enum FetchInputsResult {
    Lack,
    Next,
    Enough,
}

fn fetch_more_inputs(
    cells_iter: &mut IntoIter<InputInfo>,
    inputs: &mut Vec<InputInfo>,
    inputs_limit: usize,
    output_min_bytes: u32,
    expected_input_size: usize,
) -> Result<FetchInputsResult> {
    let mut last = false;
    if let Some(cell) = cells_iter.next() {
        inputs.push(cell);
    } else {
        if inputs.is_empty() {
            return Ok(FetchInputsResult::Lack);
        }
        last = true;
    }

    let total = inputs
        .iter()
        .map(|input| input.cell_info.capacity)
        .try_fold(core::Capacity::zero(), |total, cap| total.safe_add(cap))
        .map_err(Error::runtime)?
        .as_u64();

    if total < (u64::from(output_min_bytes) + 1) * BYTE_SHANNONS {
        let res = if last {
            FetchInputsResult::Lack
        } else {
            FetchInputsResult::Next
        };
        return Ok(res);
    }

    if last || inputs.len() >= std::cmp::min(expected_input_size, inputs_limit) {
        Ok(FetchInputsResult::Enough)
    } else {
        Ok(FetchInputsResult::Next)
    }
}

fn prepare_inputs(total_inputs: &mut Vec<InputInfo>) -> (Vec<H256>, Vec<InputInfo>) {
    total_inputs.sort_by_key(|input| input.cell_info.lock_hash.clone());
    let mut inputs_ext = total_inputs
        .drain(..)
        .fold((Vec::new(), None), |(mut result, prev), input| {
            let curr = Some(input.cell_info.lock_hash.clone());
            let is_first = prev != curr;
            result.push((is_first, input));
            (result, curr)
        })
        .0;
    inputs_ext.sort_by_key(|(is_first, input)| (!is_first, input.cell_info.lock_hash.clone()));
    let lock_hashes = inputs_ext
        .iter()
        .filter(|(is_first, _)| *is_first)
        .map(|(_, input)| input.cell_info.lock_hash.to_owned())
        .collect::<Vec<_>>();
    let inputs = inputs_ext
        .into_iter()
        .map(|(_, input)| input)
        .collect::<Vec<_>>();
    (lock_hashes, inputs)
}

fn calculate_outputs_count(
    outputs_limit: usize,
    total_shannons: u64,
    output_shannons: u64,
    fee_shannons: u64,
) -> usize {
    match ((total_shannons - fee_shannons) / output_shannons) as usize {
        0 => 1,
        x if x < outputs_limit => x,
        _ => outputs_limit,
    }
}

fn construct_raw_transaction(
    inputs_info: &[InputInfo],
    accounts: &HashMap<H256, LockInfo>,
    lock_generator: &LockGenerator,
    lock_deps_dict: &HashMap<LockScriptId, Vec<packed::CellDep>>,
    outputs_limit: usize,
    output_bytes: u32,
    fee_shannons: u64,
) -> Result<packed::RawTransaction> {
    let inputs = inputs_info
        .iter()
        .map(|input| {
            packed::CellInput::new_builder()
                .previous_output(input.out_point.clone())
                .build()
        })
        .collect::<Vec<_>>();
    let inputs_cap = inputs_info
        .iter()
        .try_fold(core::Capacity::zero(), |total, next| {
            total.safe_add(next.cell_info.capacity)
        })
        .map_err(Error::runtime)?;
    let cell_deps = inputs_info
        .iter()
        .try_fold(Vec::new(), |mut ids, input| {
            accounts.get(&input.cell_info.lock_hash).map(|account| {
                ids.push(account.id);
                ids
            })
        })
        .map(|mut ids| {
            ids.sort();
            ids.dedup();
            ids
        })
        .ok_or_else(|| Error::runtime("a lock script doesn't set"))?
        .into_iter()
        .try_fold(Vec::new(), |mut cell_deps, id| {
            lock_deps_dict.get(&id).map(|ref cds| {
                cell_deps.extend_from_slice(&cds[..]);
                cell_deps
            })
        })
        .map(|mut cell_deps| {
            cell_deps.sort_by(|x, y| x.as_slice().partial_cmp(y.as_slice()).unwrap());
            cell_deps.dedup();
            cell_deps
        })
        .ok_or_else(|| Error::runtime("a lock script doesn't have cell deps"))?;
    let outputs = {
        let mut tmp_cap = inputs_cap.as_u64();
        let output_shannons = u64::from(output_bytes) * BYTE_SHANNONS;
        let outputs_count =
            calculate_outputs_count(outputs_limit, tmp_cap, output_shannons, fee_shannons);
        let tmp_output_cap = core::Capacity::shannons(output_shannons).pack();
        let tmp_output = packed::CellOutput::new_builder()
            .capacity(tmp_output_cap)
            .build();
        let mut outputs = vec![tmp_output; outputs_count];
        tmp_cap -= fee_shannons;
        tmp_cap -= output_shannons * (outputs_count as u64 - 1);
        outputs[0] = outputs[0]
            .clone()
            .as_builder()
            .capacity((core::Capacity::shannons(tmp_cap)).pack())
            .build();
        let locks = (1..=outputs.len())
            .map(|_| lock_generator.generate())
            .map(|hash| &accounts[&hash]);
        for (index, lock) in locks.into_iter().enumerate() {
            outputs[index] = outputs[index]
                .clone()
                .as_builder()
                .lock(lock.script.to_owned())
                .build();
        }
        (&mut outputs[1..]).sort_by_key(|output| output.lock().as_slice().to_vec());
        outputs
    };
    let raw = packed::RawTransaction::new_builder()
        .inputs(inputs.pack())
        .cell_deps(cell_deps.pack())
        .outputs_data(vec![(&[]).pack(); outputs.len()].pack())
        .outputs(outputs.pack())
        .build();
    Ok(raw)
}

fn sign_transaction(
    raw_tx: packed::RawTransaction,
    lock_hashes: &[H256],
    accounts: &HashMap<H256, LockInfo>,
) -> Result<packed::Transaction> {
    let blank_signature = bytes::Bytes::from(vec![0u8; 65]);
    let witness_blank = packed::WitnessArgs::new_builder()
        .lock(Some(blank_signature).pack())
        .build();
    let tx = packed::Transaction::new_builder()
        .raw(raw_tx)
        .witnesses(vec![witness_blank.as_bytes().pack(); lock_hashes.len()].pack())
        .build();

    let tx_hash = tx.calc_tx_hash();
    let witnesses = lock_hashes
        .iter()
        .map(|hash| {
            let lock_info = &accounts[&hash];
            let signature = lock_info.sign(&tx_hash.raw_data())?;
            let witness_args = packed::WitnessArgs::new_builder()
                .lock(Some(bytes::Bytes::from(signature)).pack())
                .build()
                .as_bytes()
                .pack();
            Ok(witness_args)
        })
        .collect::<Result<Vec<_>>>()?;
    let stx = tx.as_builder().witnesses(witnesses.pack()).build();
    Ok(stx)
}
