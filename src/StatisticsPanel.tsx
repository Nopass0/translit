import {useEffect,useState} from 'react';
import {invoke,isTauri} from '@tauri-apps/api/core';
import type {ContextTranslation} from './types';
type Phrase={phrase:string;count:number;contexts:string[];game:string;patterns:string[];analysis:ContextTranslation};
type Statistics={total_queries:number;phrases:Phrase[];constructions:[string,number][]};
export function StatisticsPanel({onRefresh,onMessage}:{onRefresh:()=>Promise<void>;onMessage:(text:string,error?:boolean)=>void}){
 const [stats,setStats]=useState<Statistics>({total_queries:0,phrases:[],constructions:[]});
 const [busy,setBusy]=useState('');
 useEffect(()=>{if(isTauri())void invoke<Statistics>('get_statistics').then(setStats).catch(e=>onMessage(String(e),true));},[]);
 async function add(phrase:Phrase){setBusy(phrase.phrase);try{await invoke('save_entry',{entry:{word:phrase.phrase,translation:phrase.analysis.translation,context:phrase.contexts.at(-1)??'',game:phrase.game,created_at:0,analysis:phrase.analysis,query_count:phrase.count}});await onRefresh();onMessage('Карточка сохранена');}catch(e){onMessage(String(e),true);}finally{setBusy('');}}
 return <section className="statistics-panel"><div className="statistics-heading"><h2>Что вы спрашиваете чаще</h2><span>{stats.total_queries} обращений</span></div><div className="construction-stats">{stats.constructions.map(([label,count])=><span key={label}>{label}<b>{count}</b></span>)}</div>{stats.phrases.length?<div className="phrase-stats">{stats.phrases.slice(0,20).map(phrase=><article key={phrase.phrase}><div><strong>{phrase.phrase}</strong><p>{phrase.analysis.translation}</p><small>{phrase.patterns.join(' · ')}</small></div><b>{phrase.count}×</b><button className="secondary" disabled={Boolean(busy)||!phrase.analysis.translation} onClick={()=>void add(phrase)}>В карточки</button></article>)}</div>:<p>Счётчики появятся после первых переводов. Они сохраняются между запусками.</p>}</section>;
}
