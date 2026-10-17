import type {ClockStatus} from './shared/api';
import type {PresetAction} from './shared/settings';
export interface PresetRequest {sessionId:string; baseRevision:number; requestId:string; action:PresetAction}
export interface PresetReply {
  status:ClockStatus;
  requestId:string;
  operation:{state:'saved'|'failed'|'pending'|'rejected'; id?:string; code?:string; uncertain?:boolean};
}
// Preserve the identity of an uncertain operation: retry asks for its receipt,
// and never blindly creates a second preset after an IPC/storage timeout.
export class PresetClient {
  private pending:PresetRequest | undefined;
  constructor(private readonly getStatus:()=>Promise<ClockStatus>,
    private readonly send:(request:PresetRequest)=>Promise<PresetReply>,
    private readonly accept:(status:ClockStatus)=>ClockStatus){}
  async edit(action:PresetAction):Promise<{status:ClockStatus;id:string}>{
    if(this.pending && JSON.stringify(this.pending.action)!==JSON.stringify(action)){
      const prior=await this.send(this.pending);this.accept(prior.status);
      if(prior.requestId!==this.pending.requestId || prior.operation.state==='pending' || prior.operation.uncertain)
        throw new Error('前のプリセット操作の結果が未確定です。同じ操作で結果を確認してください。');
      this.pending=undefined;
    }
    if(!this.pending){
      const {sessionId,revision}= (await this.getStatus()).settingsSync;
      this.pending={sessionId,baseRevision:revision,requestId:crypto.randomUUID(),action};
    }
    const reply=await this.send(this.pending);
    const status=this.accept(reply.status);
    if(reply.requestId!==this.pending.requestId) throw new Error('プリセット操作の応答が一致しません。');
    if(reply.operation.state==='saved' && reply.operation.id){
      this.pending=undefined;return {status,id:reply.operation.id};
    }
    if(reply.operation.state==='rejected' || (reply.operation.state==='failed'&&!reply.operation.uncertain)){
      this.pending=undefined;
      throw new Error(`保存を確定できませんでした。プリセット操作は反映していません。${reply.operation.code ?? ''}`);
    }
    throw new Error(`保存結果は未確定です。保存済みとして扱わず、再実行による重複を避けてください。${reply.operation.code ?? ''}`);
  }
}
