import bird from '../icons/tabler/canary.svg?raw';
import church from '../icons/tabler/building-church.svg?raw';
import clock from '../icons/pendulum-clock.svg?raw';
import bell from '../icons/tabler/school-bell.svg?raw';
import music from '../icons/tabler/music.svg?raw';

function icon(id:string):SVGElement {
 const markup=({'builtin:cuckoo-clock':bird,
  'builtin:church-bell':church,'builtin:pendulum-clock':clock,'builtin:school-bell':bell} as Record<string,string>)[id]??music;
 // Only bundled, trusted SVG assets are parsed; names are always textContent.
 const svg=new DOMParser().parseFromString(markup,'image/svg+xml').documentElement;
 svg.setAttribute('aria-hidden','true');svg.setAttribute('focusable','false');svg.setAttribute('stroke-width','1.75');
 return document.importNode(svg,true) as unknown as SVGElement;
}

export class CustomSelect {
 private button=document.createElement('button');
 private popup=document.createElement('div');
 private active=-1;private signature='';private open=false;
 constructor(private select:HTMLSelectElement,private options:{icons?:boolean;placeholder?:string}={}){
  select.hidden=true;select.tabIndex=-1;select.setAttribute('aria-hidden','true');
  this.button.id=select.id+'-control';this.button.type='button';this.button.className='sound-select-trigger';
  this.button.setAttribute('role','combobox');this.button.setAttribute('aria-haspopup','listbox');this.button.setAttribute('aria-expanded','false');
  this.popup.id=select.id+'-options';this.popup.className='sound-select-popup';this.popup.setAttribute('role','listbox');this.popup.setAttribute('popover','manual');
  const label=document.querySelector<HTMLLabelElement>(`label[for="${select.id}"]`);
  if(label){label.htmlFor=this.button.id;if(!label.id)label.id=select.id+'-label';this.button.setAttribute('aria-labelledby',label.id+' '+this.button.id+'-value');this.popup.setAttribute('aria-labelledby',label.id);}
  this.button.setAttribute('aria-controls',this.popup.id);
  select.before(this.button);document.body.append(this.popup);
  this.button.addEventListener('click',()=>this.open?this.close():this.show());
  this.button.addEventListener('keydown',event=>this.key(event));
  document.addEventListener('pointerdown',event=>{if(this.open&&!this.popup.contains(event.target as Node)&&!this.button.contains(event.target as Node))this.close();});
  document.addEventListener('focusin',event=>{if(this.open&&!this.popup.contains(event.target as Node)&&event.target!==this.button)this.close();});
  const close=()=>this.close();window.addEventListener('resize',close);document.addEventListener('scroll',event=>{if(!this.popup.contains(event.target as Node))this.close();},true);
  new MutationObserver(()=>{if(document.documentElement.dataset.minimum==='true')this.close();}).observe(document.documentElement,{attributes:true,attributeFilter:['data-minimum']});
  this.sync();
 }
 sync():void{
  this.button.disabled=this.select.disabled||this.select.matches(':disabled');
  if(this.button.disabled)this.close();
  const selected=this.select.selectedOptions[0];
  const key=JSON.stringify([...this.select.options].map(o=>[o.value,o.text,o.disabled]))+this.select.value;
  if(key===this.signature)return;this.signature=key;
  const value=document.createElement('span');value.id=this.button.id+'-value';value.className='sound-select-name';value.textContent=selected?.text??this.options.placeholder??'音色';
  if(!this.button.hasAttribute('aria-labelledby'))this.button.setAttribute('aria-label',`${this.select.getAttribute('aria-label')??'選択'}：${value.textContent}`);
  if(!this.popup.hasAttribute('aria-labelledby'))this.popup.setAttribute('aria-label',this.select.getAttribute('aria-label')??'選択');
  const chevron=document.createElement('span');chevron.className='sound-select-chevron';chevron.setAttribute('aria-hidden','true');
  const arrow=document.createElementNS('http://www.w3.org/2000/svg','svg');arrow.setAttribute('viewBox','0 0 12 12');
  const path=document.createElementNS(arrow.namespaceURI,'path');path.setAttribute('d','m2.5 4.5 3.5 3 3.5-3');arrow.append(path);chevron.append(arrow);
  this.button.replaceChildren(...(this.options.icons===false?[]:[icon(this.select.value)]),value,chevron);this.button.title=selected?.text??'';
  this.button.dataset.unavailable=String(!!selected?.disabled);
  this.popup.replaceChildren(...[...this.select.options].map((option,index)=>{
   const row=document.createElement('div');row.id=this.popup.id+'-'+index;row.className='sound-select-option';row.setAttribute('role','option');
   row.setAttribute('aria-selected',String(option.selected));row.setAttribute('aria-disabled',String(option.disabled));row.title=option.text;
   const name=document.createElement('span');name.className='sound-select-name';name.textContent=option.text;
   const check=document.createElement('span');check.className='sound-select-check';check.textContent=option.selected?'✓':'';check.setAttribute('aria-hidden','true');
   row.append(...(this.options.icons===false?[]:[icon(option.value)]),name,check);
   row.addEventListener('pointerdown',event=>event.preventDefault());row.addEventListener('click',()=>this.commit(index));return row;
  }));
  if(this.open)this.close();
 }
 private show():void{
  if(this.button.matches(':disabled'))return;
  this.open=true;this.popup.showPopover();this.button.setAttribute('aria-expanded','true');
  const r=this.button.getBoundingClientRect();
  this.popup.style.setProperty('--trigger-width',r.width+'px');
  const height=this.popup.getBoundingClientRect().height;
  this.popup.style.left=Math.max(8,Math.min(r.left,innerWidth-this.popup.offsetWidth-8))+'px';
  this.popup.style.top=(innerHeight-r.bottom>=height+8?r.bottom+4:Math.max(8,r.top-height-4))+'px';
  this.active=this.select.selectedIndex;if(this.select.options[this.active]?.disabled)this.active=this.enabled()[0]??-1;
  this.highlight();
 }
 private close():void{this.open=false;if(this.popup.matches(':popover-open'))this.popup.hidePopover();this.button.setAttribute('aria-expanded','false');this.button.removeAttribute('aria-activedescendant');}
 private enabled():number[]{return [...this.select.options].flatMap((o,i)=>o.disabled?[]:[i]);}
 private highlight():void{
  [...this.popup.children].forEach((row,i)=>row.classList.toggle('active',i===this.active));
  const row=this.popup.children[this.active] as HTMLElement|undefined;
  if(row){this.button.setAttribute('aria-activedescendant',row.id);row.scrollIntoView({block:'nearest'});}
 }
 private commit(index:number):void{
  if(this.button.matches(':disabled')||!this.select.options[index]||this.select.options[index].disabled)return;
  const id=this.select.options[index].value;this.close();this.button.focus({preventScroll:true});
  if(id!==this.select.value){this.select.value=id;this.select.dispatchEvent(new Event('change',{bubbles:true}));}
 }
 private key(event:KeyboardEvent):void{
  if(event.key==='Escape'){if(this.open){event.preventDefault();event.stopPropagation();this.close();}return;}
  if(event.key==='Tab'){this.close();return;}
  if(['ArrowDown','ArrowUp','Home','End'].includes(event.key)){
   event.preventDefault();if(!this.open){this.show();return;}
   const enabled=this.enabled(),at=enabled.indexOf(this.active);
   this.active=event.key==='Home'?enabled[0]??-1:event.key==='End'?enabled.at(-1)??-1:enabled[(at+(event.key==='ArrowDown'?1:enabled.length-1))%enabled.length]??-1;this.highlight();
  }else if(event.key==='Enter'||event.key===' '){event.preventDefault();if(this.open)this.commit(this.active);else this.show();}
 }
 focus():void {this.button.focus();}
}
export {CustomSelect as SoundSelect};
