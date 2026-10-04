import {useEffect,useMemo,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {BookOpen,ChevronLeft,ChevronRight,Check,RefreshCw,Eye,CalendarDays} from 'lucide-react';
import type {Entry} from './types';
import {GrammarView} from './GrammarView';
import {StatisticsPanel} from './StatisticsPanel';

export function Study({entries,paused,onRefresh,onMessage,onBack}:{entries:Entry[];paused:boolean;onRefresh:()=>Promise<void>;onMessage:(text:string,error?:boolean)=>void;onBack:()=>void}){
 const [index,setIndex]=useState(0),[revealed,setRevealed]=useState(false),[busy,setBusy]=useState(false);
 const [today,setToday]=useState(true),[game,setGame]=useState(''),[now,setNow]=useState(Date.now()/1000);
 const games=useMemo(()=>Array.from(new Set(entries.map(e=>e.game).filter(Boolean))).sort(),[entries]);
 const filtered=useMemo(()=>entries.filter(e=>!game||e.game===game),[entries,game]);
 const due=useMemo(()=>filtered.filter(e=>(e.due_at??0)<=now).sort((a,b)=>(a.due_at??0)-(b.due_at??0)||(b.query_count??0)-(a.query_count??0)),[filtered,now]);
 const list=today?due:filtered;
 const latest=useRef({list,index,revealed,busy,paused,today});latest.current={list,index,revealed,busy,paused,today};
 const entry=list[Math.min(index,Math.max(0,list.length-1))];
 const upcoming=filtered.map(e=>e.due_at??0).filter(t=>t>now).sort((a,b)=>a-b)[0];
 function next(delta:number){const cards=latest.current.list;if(!cards.length)return;setIndex(i=>(i+delta+cards.length)%cards.length);setRevealed(false);}
 function changeMode(value:boolean){setToday(value);setIndex(0);setRevealed(false);}
 async function review(remembered:boolean){
  const current=latest.current;if(current.busy||!current.list.length||!current.revealed)return;
  setBusy(true);
  try{
   await invoke('review_entry',{word:current.list[Math.min(current.index,current.list.length-1)].word,learned:remembered});
   await onRefresh();setNow(Date.now()/1000);
   if(current.today)setIndex(0);else next(1);
   setRevealed(false);
   onMessage(remembered?'Повторение запланировано':'Вернёмся к выражению через 10 минут');
  }catch(e){onMessage(String(e),true);}finally{setBusy(false);}
 }
 useEffect(()=>{const timer=setInterval(()=>setNow(Date.now()/1000),30000);return()=>clearInterval(timer);},[]);
 useEffect(()=>{
  const key=(e:KeyboardEvent)=>{
   if((e.target as HTMLElement)?.matches('input,textarea,select'))return;
   if(e.key==='ArrowRight'){e.preventDefault();next(1);}
   if(e.key==='ArrowLeft'){e.preventDefault();next(-1);}
   if(e.key===' '||e.key==='Enter'){e.preventDefault();setRevealed(v=>!v);}
   if(e.key.toLowerCase()==='x'||e.key==='2')void review(true);
   if(e.key.toLowerCase()==='y'||e.key==='1')void review(false);
  };
  window.addEventListener('keydown',key);
  const sub=listen<string>('controller-action',e=>{
   if(['left','up'].includes(e.payload))next(-1);
   if(['right','down'].includes(e.payload))next(1);
   if(e.payload==='choose')setRevealed(v=>!v);
   if(e.payload==='save')void review(true);
   if(e.payload==='mode')void review(false);
   if(e.payload==='resume'){if(latest.current.paused)void invoke('resume_game');else onBack();}
  });
  return()=>{window.removeEventListener('keydown',key);void sub.then(f=>f());};
 },[]);
 return <section className="study-page">
  <div className="study-filters"><div><button className={today?'on':''} onClick={()=>changeMode(true)}><CalendarDays size={15}/>К повторению <b>{due.length}</b></button><button className={!today?'on':''} onClick={()=>changeMode(false)}>Все карточки <b>{filtered.length}</b></button></div><select aria-label="Карточки по игре" value={game} onChange={e=>{setGame(e.target.value);setIndex(0);setRevealed(false);}}><option value="">Все игры</option>{games.map(g=><option key={g}>{g}</option>)}</select></div>
  {entry?<>
   <div className="study-toolbar"><span><BookOpen size={17}/>{Math.min(index+1,list.length)} / {list.length}</span><span>{entries.reduce((sum,e)=>sum+(e.review_count??0),0)} повторений</span><div><button className="icon-button" onClick={()=>next(-1)} aria-label="Предыдущая карточка"><ChevronLeft size={19}/></button><button className="icon-button" onClick={()=>next(1)} aria-label="Следующая карточка"><ChevronRight size={19}/></button></div></div>
   <article className="study-card"><span className="eyebrow mint">ВОСПОМНИТЕ ЗНАЧЕНИЕ В ЭТОЙ СИТУАЦИИ</span><h2>{entry.word}</h2>{entry.context&&<blockquote>{entry.context}</blockquote>}
    {!revealed?<button className="primary" onClick={()=>setRevealed(true)}><Eye size={17}/>Показать перевод <kbd>A / Space</kbd></button>:<div className="study-answer"><h3>{entry.translation}</h3>{entry.analysis&&<><p>{entry.analysis.explanation}</p><GrammarView analysis={entry.analysis}/>{entry.analysis.context_translation&&<blockquote>{entry.analysis.context_translation}</blockquote>}{entry.analysis.grammar&&<div className="grammar-panel"><h3>{entry.analysis.construction||'Грамматика'}</h3><p>{entry.analysis.grammar}</p></div>}{Boolean(entry.analysis.examples?.length)&&<div className="grammar-panel"><h3>Примеры</h3>{entry.analysis.examples?.map((example,i)=><p key={i}>{example}</p>)}</div>}</>}<div className="study-actions"><button className="secondary" disabled={busy} onClick={()=>void review(false)}><RefreshCw size={15}/>Ещё повторить <kbd>Y / 1</kbd></button><button className="primary" disabled={busy} onClick={()=>void review(true)}><Check size={17}/>Помню <kbd>X / 2</kbd></button></div></div>}
    <small>{entry.game} · повторений: {entry.review_count??0} · запросов: {entry.query_count??0} · ошибок: {entry.lapses??0}{Boolean(entry.due_at)&&` · следующее: ${new Date(entry.due_at!*1000).toLocaleString('ru-RU')}`}</small>
   </article><div className="shortcut-strip"><span><kbd>← → / D-pad</kbd>Карточка</span><span><kbd>A / Space</kbd>Перевод</span><span><kbd>X</kbd>Помню</span><span><kbd>Y</kbd>Повторить</span></div>
  </>:<div className="empty-dictionary"><Check size={46}/><h2>{entries.length?'На сегодня всё':'Сохраните первое выражение'}</h2><p>{upcoming?`Следующее повторение: ${new Date(upcoming*1000).toLocaleString('ru-RU')}`:'Сохраняйте слова и выражения вместе с игровыми репликами.'}</p>{entries.length>0&&<button className="secondary" onClick={()=>changeMode(false)}>Посмотреть все карточки</button>}</div>}
  <p className="study-schedule-note">При правильном ответе: 1 → 3 → 7 → 14 → 30 → 60 → 120 → 240 дней. При ошибке — через 10 минут. Расписание сохраняется на этом компьютере.</p>
  <StatisticsPanel onRefresh={onRefresh} onMessage={onMessage}/>
 </section>;
}
