export class WindowControls {
 private reading=false;
 private dragging=false;
 private sequence=0;
 constructor(){
  const caption=document.querySelector<HTMLElement>('#window-caption')!;
  // A modal HTML dialog makes the rest of the document inert. Keep the same
  // caption node in the active modal so native controls remain reachable.
  const bar=document.querySelector<HTMLElement>('.window-titlebar')!;
  const modal=new MutationObserver(()=>{const host=document.querySelector('dialog:modal')??document.body;if(bar.parentElement!==host)host.prepend(bar);});
  modal.observe(document.documentElement,{subtree:true,attributes:true,attributeFilter:['open']});
  window.addEventListener('unload',()=>modal.disconnect(),{once:true});
  caption.addEventListener('mousedown',e=>{if(e.button===0&&e.detail===1&&!this.dragging){e.preventDefault();this.dragging=true;void this.action('drag').finally(()=>this.dragging=false)}});
  caption.addEventListener('dblclick',()=>void this.action('maximize'));
  for(const[id,action]of [['window-minimize','minimize'],['window-maximize','maximize'],['window-close','close']]as const)document.getElementById(id)!.addEventListener('click',()=>void this.action(action));
  window.addEventListener('resize',()=>void this.read());void this.read();
 }
 private async action(action:'drag'|'minimize'|'maximize'|'close'):Promise<void>{const n=++this.sequence;try{const s=await window.cuckoo.windowAction(action);if(n===this.sequence)this.update(s);}catch(e){if(action!=='close'&&n===this.sequence)document.querySelector('#input-error')!.textContent=`ウィンドウ操作に失敗しました：${String(e)}`;}}
 private async read():Promise<void>{if(this.reading)return;this.reading=true;const n=++this.sequence;try{const s=await window.cuckoo.windowAction('state');if(n===this.sequence)this.update(s);}catch{/* Window may already be closing. */}finally{this.reading=false;}}
 private update(s:{maximized:boolean}):void{const b=document.getElementById('window-maximize')!;b.setAttribute('aria-label',s.maximized?'元のサイズに戻す':'最大化');b.setAttribute('title',s.maximized?'元のサイズに戻す':'最大化');b.classList.toggle('is-maximized',s.maximized);}
}
