declare module '*.css';
interface Window {
  cuckoo: import('./shared/api').CuckooApi;
  cuckooAudio: import('./shared/api').AudioApi;
}
declare module '*.svg' {
  const source: string;
  export default source;
}
declare module '*.wav' {
  const url: string;
  export default url;
}
