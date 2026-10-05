import {useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import type {Status} from './types';
import {defaults} from './types';
import './overlay.css';
type Caption={source:string;translation:string;};
export function LiveCaptions(){
 const [caption,setCaption]=useState<Caption|null>(null);const[status,setStatus]=useState<Status>(defaults);const[font,setFont]=useState(30);
 useEffect(()=>{document.documentElement.classList.add("overlay-document");document.body.classList.add("overlay-body");void invoke<Status>('get_status').then(setStatus).catch(()=>{});const events=[listen<Caption|null>('live-caption',e=>setCaption(e.payload)),listen('status-changed',()=>void invoke<Status>('get_status').then(setStatus).catch(()=>{})),listen<number>('caption-style',e=>setFont(Math.min(56,Math.max(18,e.payload))))];return()=>events.forEach(p=>void p.then(f=>f()));},[]);
 return <main className="live-captions" aria-live="polite">{caption&&<section className="live-caption-card" style={{top:`${Math.max(16,Math.min(78,status.settings.subtitles.region_top*100-12))}%`,maxHeight:`${98-Math.max(16,Math.min(78,status.settings.subtitles.region_top*100-12))}vh`,overflowY:"auto"}}><small>TRANSLIT · LIVE</small><div className="live-caption-translation" style={{fontSize:font}}>{caption.translation}</div>{caption.source&&<div className="live-caption-source">{caption.source}</div>}</section>}</main>;
}
