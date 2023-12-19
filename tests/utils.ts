import { AnchorProvider } from "@coral-xyz/anchor";
import type { Connection } from "@solana/web3.js";
import { parseRawMrEnclave, sleep } from "@switchboard-xyz/common";
import {
  BootstrappedAttestationQueue,
  SwitchboardProgram,
  AttestationProgramStateAccount,
  AttestationQueueAccount,
} from "@switchboard-xyz/solana.js";

export async function printLogs(
  connection: Connection,
  tx: string,
  v0Txn?: boolean,
  delay = 3000
) {
  await sleep(delay);
  const parsed = await connection.getParsedTransaction(tx, {
    commitment: "confirmed",
    maxSupportedTransactionVersion: v0Txn ? 0 : undefined,
  });
  console.log(parsed?.meta?.logMessages?.join("\n"));
}

export const unixTimestamp = () => Math.floor(Date.now() / 1000);

// Made up MrEnclave to use for our tests
export const MRENCLAVE = parseRawMrEnclave(
  "0x44e8f2f806229322780fbddff3e46dd23896e3f00d630fbf026ce36314c0fee1",
  true
);

export async function setupTest(
  provider: AnchorProvider
): Promise<BootstrappedAttestationQueue> {
  const switchboardProgram = await SwitchboardProgram.fromProvider(provider);

  await AttestationProgramStateAccount.getOrCreate(switchboardProgram);

  const switchboard = await AttestationQueueAccount.bootstrapNewQueue(
    switchboardProgram
  );

  return switchboard;
}
