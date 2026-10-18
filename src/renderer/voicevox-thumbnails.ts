// Session cache only: no images are bundled or added to speech/settings files.
const entries=new Map<string,{promise:Promise<string|null>;expires:number;bytes:number}>();
const waiting:(()=>void)[]=[];let active=0;
const MAX_BYTES=8*1024*1024;
function trim():void{
  let bytes=[...entries.values()].reduce((n,e)=>n+e.bytes,0);
  for(const [key,entry]of entries){if(entries.size<=128&&bytes<=MAX_BYTES)break;if(!Number.isFinite(entry.expires))continue;entries.delete(key);bytes-=entry.bytes;}
}
function run<T>(task:()=>Promise<T>):Promise<T>{return new Promise((resolve,reject)=>{
  const start=()=>{active++;void Promise.resolve().then(task).then(resolve,reject).finally(()=>{active--;waiting.shift()?.();});};
  if(active<3)start();else waiting.push(start);
});}
export function voicevoxThumbnail(uuid:string,styleId:number,version:string):Promise<string|null>{
  const key=JSON.stringify([uuid,styleId,version]),cached=entries.get(key);
  if(cached&&Date.now()<cached.expires){entries.delete(key);entries.set(key,cached);return cached.promise;}
  const entry={promise:Promise.resolve<string|null>(null),expires:Infinity,bytes:0};
  entry.promise=run(()=>window.cuckoo.voicevoxThumbnail(uuid,styleId)).catch(()=>null).then(src=>{
    entry.expires=src?Infinity:Date.now()+30_000;entry.bytes=(src?.length??0)*2;
    // Mark completed entries separately from pending work for bounded LRU eviction.
    if(src)entry.expires=Number.MAX_SAFE_INTEGER;
    trim();return src;
  });
  entries.set(key,entry);trim();return entry.promise;
}
