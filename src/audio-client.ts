export interface AudioReceipt {
 id:number;
 kind?:'Test'|'Automatic'|'Probe';
 soundId?:string;
 phase:string;
 outcome:null|{state:'Completed'|'Interrupted'|'Failed'|'Skipped';code?:string};
}
// Acceptance is not completion. Each IPC wait yields without holding Rust locks.
export async function waitForPlayback(id:number,read:(id:number)=>Promise<AudioReceipt|null>,pause=()=>new Promise<void>(r=>setTimeout(r,100))):Promise<void>{
 for(;;){
  const receipt=await read(id);
  if(!receipt)throw new Error('再生結果を確認できません。');
  if(receipt.phase==='Stuck')throw new Error('音声処理の停止を確認できません。');
  if(receipt.outcome){
   if(receipt.outcome.state==='Completed')return;
   if(receipt.outcome.state==='Interrupted')throw new Error(`再生を中断しました：${receipt.outcome.code}`);
   throw new Error(`再生エラー：${receipt.outcome.code ?? receipt.outcome.state}`);
  }
  await pause();
 }
}
