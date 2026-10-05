import {useEffect,useState} from 'react';
import {invoke,isTauri} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {BrainCircuit,Download,Play,Square,Check,KeyRound,RefreshCw} from 'lucide-react';
import type {Settings,Translator} from './types';

type Mini={installed:boolean;running:boolean;ready:boolean;parser_only:boolean;model:string;size_mb:number;threads:number;device:string;grammar:boolean;installing:boolean};
 type Preset={id:string;name:string;size:number;grammar:boolean;gpu?:boolean;source:string;context_size?:number};
const opus:Preset={id:'opus',name:'OPUS-MT · EN → RU · int8',size:78276475,grammar:false,source:'https://huggingface.co/Helsinki-NLP/opus-mt-en-ru'};
export function TranslatorPanel({settings,onChange,onMessage}:{settings:Settings;onChange:(settings:Partial<Settings>)=>Promise<void>;onMessage:(text:string,error?:boolean)=>void}){
 const [draft,setDraft]=useState<Translator>(settings.translator);
 const savedTranslator=JSON.stringify(settings.translator);
 const [mini,setMini]=useState<Mini>({installed:false,running:false,ready:false,parser_only:false,model:'OPUS-MT',size_mb:75,threads:2,device:'cpu',grammar:false,installing:false});
 const [presets,setPresets]=useState<Preset[]>([opus]);
 const [busy,setBusy]=useState(false),[progress,setProgress]=useState(''),[key,setKey]=useState(''),[keySaved,setKeySaved]=useState(false);
 async function refresh(){if(isTauri()){const [model,saved]=await Promise.all([invoke<Mini>('mini_status'),invoke<boolean>('key_status')]);setMini(model);setKeySaved(saved);}}
 useEffect(()=>setDraft(JSON.parse(savedTranslator) as Translator),[savedTranslator]);
 useEffect(()=>{
  void refresh().catch(e=>onMessage(String(e),true));
  if(isTauri())void invoke<Preset[]>('model_catalog').then(models=>setPresets([opus,...models]));
  const timer=setInterval(()=>void refresh().catch(()=>{}),2000);
  const subs=[listen<{label:string;received:number;total:number}>('model-progress',e=>setProgress(`${e.payload.label}: ${Math.round(e.payload.received/1048576)} / ${Math.round(e.payload.total/1048576)} MiB`)),listen('model-ready',()=>{setProgress('');void refresh();})];
  return()=>{clearInterval(timer);subs.forEach(p=>void p.then(f=>f()));};
 },[]);
 async function job(fn:()=>Promise<void>){setBusy(true);try{await fn();await refresh();}catch(e){onMessage(String(e),true);}finally{setBusy(false);}}
 function provider(value:string){setDraft({...draft,provider:value,endpoint:value==='ollama'?'http://127.0.0.1:11434':value==='openai'?'https://api.openai.com/v1':value==='compatible'?'http://127.0.0.1:1234/v1':draft.endpoint,model:''});}
 const preset=presets.find(p=>p.id===draft.mini_model)??opus;
 const working=busy||mini.installing;
 return <section className="settings-card full translator-card">
  <BrainCircuit size={24}/><h2>Перевод, контекст и грамматика</h2>
  <p>OPUS-MT — быстрый перевод реплики. Qwen, Ollama и API — значение, грамматика, примеры и предыдущие захваченные реплики.</p>
  <div className="translator-columns"><div>
   <label className="field-label">Переводчик<select disabled={working} value={draft.provider} onChange={e=>provider(e.target.value)}>
    <option value="offline">Офлайн · словарь и выражения</option><option value="mini">Встроенные локальные модели</option><option value="ollama">Ollama · ваша локальная модель</option><option value="openai">OpenAI · Responses API</option><option value="compatible">Совместимый API / LM Studio</option>
   </select></label>
   {!['offline','mini'].includes(draft.provider)&&<>
    <label className="field-label">Адрес сервера<input disabled={working} value={draft.endpoint} onChange={e=>setDraft({...draft,endpoint:e.target.value})}/></label>
    <label className="field-label">Название доступной модели<input disabled={working} value={draft.model} onChange={e=>setDraft({...draft,model:e.target.value})} placeholder={draft.provider==='ollama'?'Название из ollama list':'ID модели вашего провайдера'}/></label>
   </>}
   <label className="field-label">Предыдущие реплики<select disabled={working} value={draft.history_lines} onChange={e=>setDraft({...draft,history_lines:Number(e.target.value)})}>{[0,2,4,6,8,12].map(n=><option key={n} value={n}>{n} реплик</option>)}</select></label>
   <div className="model-controls">
    <label className="field-label">Потоки CPU<select disabled={working} value={draft.threads} onChange={e=>setDraft({...draft,threads:Number(e.target.value)})}>{[0,1,2,4,6,8,12,16,24,32,64].map(n=><option key={n} value={n}>{n===0?'Авто · до 8':n}</option>)}</select></label>
    <label className="field-label">Ускорение<select value={draft.device} disabled={working||!preset.gpu} onChange={e=>setDraft({...draft,device:e.target.value})}><option value="cpu">CPU</option><option value="vulkan">GPU · Vulkan</option></select></label>
   </div>
   <p className="settings-note">Vulkan доступен для Qwen3 4B на совместимой NVIDIA / AMD / Intel. OPUS-MT и экспериментальные Qwen3.5 здесь используют CPU. Больше потоков может ускорить перевод, но конкурирует с игрой за процессор.</p>
   <label className="setting-toggle"><div><strong>Загружать модель при старте</strong><p>Начать загрузку в фоне сразу после запуска Translit.</p></div><input type="checkbox" disabled={working} checked={draft.preload} onChange={e=>setDraft({...draft,preload:e.target.checked})}/></label>
   <label className="setting-toggle"><div><strong>Автоматически установить выбранную модель</strong><p>Если её ещё нет — скачать при запуске или применении настроек.</p></div><input type="checkbox" disabled={working} checked={draft.auto_install} onChange={e=>setDraft({...draft,auto_install:e.target.checked})}/></label>
   <button className="primary" disabled={working||!isTauri()} onClick={()=>void job(async()=>{await onChange({translator:draft});onMessage('Настройки сохранены. Модель готовится в фоне.');})}><Check size={16}/>Применить настройки</button>
   {['openai','compatible'].includes(draft.provider)&&<div className="key-settings">
    <label className="field-label">API ключ · {keySaved?'сохранён в Windows':'не установлен'}<input type="password" autoComplete="off" value={key} onChange={e=>setKey(e.target.value)} placeholder="Windows Credential Manager"/></label>
    <button className="secondary" disabled={!key.trim()||busy} onClick={()=>void job(async()=>{await invoke('save_api_key',{key});setKey('');onMessage('Ключ сохранён в Windows');})}><KeyRound size={15}/>Сохранить ключ</button>
    {keySaved&&<button className="text-button" onClick={()=>void job(async()=>{await invoke('save_api_key',{key:''});})}>Удалить ключ</button>}
   </div>}
   {draft.provider==='openai'&&<p className="settings-note">В облако передаётся текст и контекст. Кадр игры не отправляется.</p>}
  </div><div className="mini-card">
   <div className="mini-heading"><span className="mini-symbol"><BrainCircuit size={27}/></span><div><span className="eyebrow mint">ЛОКАЛЬНО · БЕЗ ПОДПИСКИ</span><h3>Выберите свою модель</h3></div></div>
   <div className="model-presets">{presets.map(model=><button disabled={working} key={model.id} className={'model-preset '+(draft.mini_model===model.id?'selected':'')} onClick={()=>setDraft({...draft,mini_model:model.id,provider:'mini',device:model.gpu?draft.device:'cpu'})}>
    <strong>{model.name}</strong><span>{Math.round(model.size/1048576)} MiB весов · {model.grammar?'перевод, контекст, грамматика':'быстрый перевод · слабые ПК'}</span>
   </button>)}</div>
   <p>{preset.id==='bonsai-1.7b'?'Bonsai: компактный контекст 1024 токена, короткие пояснения и перевод. При неполном ответе используется OPUS. Квантование Q1 может снижать точность.':preset.grammar?'Qwen анализирует значение, конструкции и примеры. 4B требует больше памяти. Свежие 0.8B и 2B — экспериментальные: качество грамматики ниже. Разбор можно отредактировать перед сохранением.':'Компактная модель, обученная переводу. Переводит реплику целиком и приблизительно связывает выбранные слова с русским текстом. Синтаксис отдельно разбирает встроенная spaCy: роли слов, V1/V2/V3 и формы неправильных глаголов.'}</p>
   <p className="settings-note">Размер весов не равен расходу RAM / VRAM. Runtime устанавливается автоматически. <a href={preset.source} target="_blank" rel="noreferrer">Описание модели</a></p>
   {progress&&mini.installing&&<div className="model-progress"><RefreshCw size={14} className="spin"/>{progress}</div>}
   <div className="mini-actions">
    <span className="model-state"><span className={'dot '+(mini.running?'green':'')}/>{mini.installing?'Скачиваю модель…':mini.parser_only?'Работает грамматический анализатор':mini.ready?'Готова к переводу':mini.running?'Загружаю веса модели…':mini.installed?'Установлена · выгружена':'Выбранная модель не установлена'}</span>
    <button className="primary" disabled={working} onClick={()=>void job(async()=>{await onChange({translator:{...draft,provider:'mini',preload:false,auto_install:false}});await invoke('install_mini');await onChange({translator:{...draft,provider:'mini'}});await invoke('start_mini');onMessage('Модель установлена и готова к переводу');})}><Download size={15}/>Установить и использовать</button>
    {mini.installed&&<button className="secondary" disabled={working} onClick={()=>void job(async()=>{await invoke(mini.running&&!mini.parser_only?'stop_mini':'start_mini');})}>{mini.running&&!mini.parser_only?<Square size={14}/>:<Play size={14}/>} {mini.running&&!mini.parser_only?'Выгрузить':'Загрузить'}</button>}
   </div><div className="model-status-note">Сохранённая модель: {mini.model} · {mini.threads} CPU · {mini.device==='vulkan'?'GPU Vulkan':'без GPU'}</div>
  </div></div>
 </section>;
}
