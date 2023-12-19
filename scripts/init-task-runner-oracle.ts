import { SwitchboardProgram, loadKeypair } from "@switchboard-xyz/solana.js";
import * as anchor from "@coral-xyz/anchor";
import { TaskRunnerOracle } from "../target/types/task_runner_oracle";
import dotenv from "dotenv";
import { loadDefaultQueue } from "./utils";
dotenv.config();

(async () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(
    process.argv.length > 2
      ? new anchor.AnchorProvider(
          provider.connection,
          new anchor.Wallet(loadKeypair(process.argv[2])),
          {}
        )
      : provider
  );

  const payer = (provider.wallet as anchor.Wallet).payer;
  console.log(`PAYER: ${payer.publicKey}`);

  const program: anchor.Program<TaskRunnerOracle> =
    anchor.workspace.TaskRunnerOracle;
  console.log(`TaskRunnerOracle Program ID: ${program.programId}`);

  const switchboardProgram = await SwitchboardProgram.fromProvider(provider);

  const [programStatePubkey] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("TaskRunnerOracle")],
    program.programId
  );
  console.log(`PROGRAM_STATE: ${programStatePubkey}`);

  try {
    const programState = await program.account.myProgramState.fetch(
      programStatePubkey
    );
    console.log(`Program state already initialized`);
    console.log(`ORACLE: ${programState.oracle}`);
    console.log(
      `PROGRAM_STATE: \n${JSON.stringify(programState, undefined, 2)}`
    );
    return;

    // Account already initialized
  } catch (error) {
    if (!`${error}`.includes("Account does not exist or has no data")) {
      throw error;
    }
  }

  const attestationQueueAccount = await loadDefaultQueue(switchboardProgram);
  console.log(`ATTESTATION_QUEUE: ${attestationQueueAccount.publicKey}`);

  // Create the instructions to initialize our Switchboard Function
  const [functionAccount, functionInit] =
    await attestationQueueAccount.createFunctionInstruction(payer.publicKey, {
      schedule: "15 * * * * *",
      container: `${process.env.DOCKERHUB_ORGANIZATION ?? "switchboardlabs"}/${
        process.env.DOCKERHUB_CONTAINER_NAME ?? "solana-task-runner-function"
      }`,
      version: `${process.env.DOCKERHUB_CONTAINER_VERSION ?? "typescript"}`, // TODO: set to 'latest' after testing
    });
  console.log(`SWITCHBOARD_FUNCTION: ${functionAccount.publicKey}`);

  const oracleKeypair = await anchor.web3.Keypair.generate();
  const space = program.account.myOracleState.size;

  const signature = await program.methods
    .initialize()
    .accounts({
      program: programStatePubkey,
      oracle: oracleKeypair.publicKey,
      authority: payer.publicKey,
      switchboardFunction: functionAccount.publicKey,
    })
    .signers([...functionInit.signers])
    .preInstructions([
      ...functionInit.ixns,
      // oracle account is a keypair account so theres no PDA size restrictions
      anchor.web3.SystemProgram.createAccount({
        fromPubkey: payer.publicKey,
        newAccountPubkey: oracleKeypair.publicKey,
        lamports:
          await program.provider.connection.getMinimumBalanceForRentExemption(
            space
          ),
        space,
        programId: program.programId,
      }),
    ])
    .rpc();

  console.log(`[TX] initialize: ${signature}`);
})();
