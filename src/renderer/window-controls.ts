export class WindowControls {
 private dragging=false;
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
  document.getElementById('window-close')!.addEventListener('click',()=>void this.action('close'));
 }
 private async action(action:'drag'|'minimize'|'maximize'|'close'):Promise<void>{try{await window.cuckoo.windowAction(action);}catch(e){if(action!=='close')document.querySelector('#input-error')!.textContent=`ウィンドウ操作に失敗しました：${String(e)}`;}}

}
