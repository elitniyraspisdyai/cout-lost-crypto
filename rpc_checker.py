#!/usr/bin/env python3
import subprocess, json, sys, logging
from bip_utils import Bip39SeedGenerator, Bip44, Bip44Coins, Bip44Changes

# Настройка логирования
logging.basicConfig(level=logging.INFO, format='[%(levelname)s] %(message)s')

# RPC-конфиг (подставьте свои данные)
RPC_USER = 'user'
RPC_PASSWORD = 'pass'
RPC_PORT = 8332
RPC_HOST = 'localhost'

def rpc_call(method, params=None):
    if params is None:
        params = []
    payload = json.dumps({
        'jsonrpc': '1.0',
        'id': 'seed-brute',
        'method': method,
        'params': params
    })
    cmd = [
        'bitcoin-cli',
        f'-rpcuser={RPC_USER}',
        f'-rpcpassword={RPC_PASSWORD}',
        f'-rpcconnect={RPC_HOST}',
        f'-rpcport={RPC_PORT}',
        'call',
        payload
    ]
    try:
        output = subprocess.check_output(cmd)
        data = json.loads(output)
        return data.get('result')
    except subprocess.CalledProcessError as e:
        logging.error(f"RPC call error: {e}")
        return None

def derive_addresses(seed_phrase: str, count: int = 20) -> list[str]:
    seed_bytes = Bip39SeedGenerator(seed_phrase).Generate()
    bip44_mst = Bip44.FromSeed(seed_bytes, Bip44Coins.BITCOIN)
    addresses = []
    for i in range(count):
        addr = (
            bip44_mst
            .Purpose()
            .Coin()
            .Account(0)
            .Change(Bip44Changes.CHAIN_EXT)
            .AddressIndex(i)
            .PublicKey()
            .ToAddress()
        )
        addresses.append(addr)
    return addresses

def check_addresses_balance(addresses: list[str]) -> tuple[str, float] | None:
    for addr in addresses:
        rpc_call('importaddress', [addr, 'temp', False])
        bal = rpc_call('getreceivedbyaddress', [addr, 0])
        if bal is None:
            logging.error(f"Ошибка при проверке баланса {addr}")
            continue
        if bal > 0:
            return addr, bal
    return None

def send_to(address, amount):
    return rpc_call('sendtoaddress', [address, amount])

def main(seed_phrase, target_address):
    logging.info(f"Checking seed: {seed_phrase}")
    addresses = derive_addresses(seed_phrase, 20)
    result = check_addresses_balance(addresses)
    if result:
        found_addr, amount = result
        logging.info(f"Found balance {amount} BTC at {found_addr}")
        txid = send_to(target_address, amount)
        logging.info(f"Sent funds, TXID: {txid}")
    else:
        logging.info("No balance found for this seed")

if __name__ == '__main__':
    if len(sys.argv) < 3:
        print("Usage: rpc_checker.py <seed_phrase> <target_address>")
        sys.exit(1)
    main(sys.argv[1], sys.argv[2])
