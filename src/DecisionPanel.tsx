import {useEffect,useState} from 'react';
import {listen} from '@tauri-apps/api/event';
import {invoke,isTauri} from '@tauri-apps/api/core';
import {BrainCircuit,Download,Play,Square} from 'lucide-react';
import type {DecisionSettings} from './types';

type Runtime={installed:boolean;running:boolean;ready:boolean;installing:boolean;error:string};
export function DecisionPanel({settings,onChange,onMessage}:{settings:DecisionSettings;onChange:(patch:{decision:DecisionSettings})=>Promise<void>;onMessage:(text:string,error?:boolean)=>void}){
 const [runtime,setRuntime]=useState<Runtime>({installed:false,running:false,ready:false,installing:false,error:''});const[busy,setBusy]=useState(false);
 async function refresh(){if(isTauri())setRuntime(await invoke<Runtime>('decision_status'));}
 useEffect(()=>{void refresh().catch(()=>{});const timer=setInterval(()=>void refresh().catch(()=>{}),1500);return()=>clearInterval(timer);},[]);
 async function perform(job:()=>Promise<void>){setBusy(true);try{await job();await refresh();}catch(e){onMessage(String(e),true);}finally{setBusy(false);}}
 const blocked=busy||runtime.installing;
 const [progress,setProgress]=useState('');
 useEffect(()=>{const subscription=listen<{label:string;received:number;total:number}>('model-progress',e=>setProgress(`${e.payload.label}: ${Math.round(e.payload.received/1048576)} / ${Math.round(e.payload.total/1048576)} МиБ`));return()=>void subscription.then(f=>f());},[]);
 function update(patch:Partial<DecisionSettings>){void onChange({decision:{...settings,...patch}}).catch(e=>onMessage(String(e),true));}
 return <section className="settings-card full decision-card"><BrainCircuit size={24}/><h2>Laya · контекстный выбор</h2>
  <p>Сопоставляет варианты FreeDict с короткой репликой, распознаёт речевое действие, переносный смысл и тон. Встроенная модель не сочиняет перевод: она ранжирует варианты, которые можно просмотреть и выбрать.</p>
  <div className="decision-status"><span className={'dot '+(runtime.ready?'green':'')}/>{runtime.installing?'Скачиваю Laya и запускаю…':runtime.ready?'Готова · значения, смысл выражения и тон':runtime.installed?'Установлена · можно запустить':'При первом старте скачает около 680 МиБ'}</div>
  <div className="decision-controls"><label className="setting-toggle"><div><strong>Показывать подсказки Laya</strong><p>Значения словаря и признаки реплики появляются в карточке.</p></div><input type="checkbox" checked={settings.enabled} onChange={e=>update({enabled:e.target.checked})}/></label>
  <label className="setting-toggle"><div><strong>Загружать при запуске</strong><p>После первой загрузки веса остаются локально; интернет нужен только для самой загрузки.</p></div><input type="checkbox" checked={settings.preload} onChange={e=>update({preload:e.target.checked})}/></label></div>
  {runtime.installing&&progress&&<div className="model-progress">{progress}</div>}
  <label className="setting-toggle"><div><strong>Автоматически скачивать Laya</strong><p>Проверяет контрольные суммы файлов перед запуском.</p></div><input type="checkbox" checked={settings.auto_install} onChange={e=>update({auto_install:e.target.checked})}/></label>
  <label className="field-label">Потоки CPU<select value={settings.threads} onChange={e=>update({threads:Number(e.target.value)})}>{[1,2,4,6,8].map(n=><option key={n} value={n}>{n}</option>)}</select></label>
  <label className="field-label">Размер контекста<select value={settings.tokens} onChange={e=>update({tokens:Number(e.target.value)})}><option value={256}>256 · минимальная задержка</option><option value={512}>512 · обычные реплики</option><option value={768}>768 · длинный диалог</option><option value={1024}>1024 · расширенный контекст</option></select></label>
  <div className="mini-actions"><span className="model-state"><span className={'dot '+(runtime.ready?'green':'')}/>{runtime.error||`${settings.threads||2} потока CPU · Laya Multilingual · ONNX`}</span>{!runtime.installed?<button className="primary" disabled={blocked||!isTauri()} onClick={()=>void perform(async()=>{await invoke('install_decision');await invoke('start_decision');onMessage('Laya установлена и готова');})}><Download size={15}/>Скачать Laya</button>:<button className="secondary" disabled={blocked||!isTauri()} onClick={()=>void perform(async()=>{await invoke(runtime.running?'stop_decision':'start_decision');})}>{runtime.running?<Square size={14}/>:<Play size={14}/>} {runtime.running?'Выгрузить Laya':'Запустить Laya'}</button>}</div>
  <small className="decision-footnote">CPU · около 0,3–0,4 с на один выбор после прогрева в проверке этого релиза; скорость зависит от компьютера. Рекомендация сохраняется в карточке и не заменяет проверку словаря.</small>
 </section>;
}
