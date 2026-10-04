import { getCurrentWindow } from '@tauri-apps/api/window';
import { isTauri } from '@tauri-apps/api/core';
import {BookOpen,Minus,Square,X} from 'lucide-react';
export function Titlebar(){
  return <div className="custom-titlebar"><div className="titlebar-brand"><BookOpen size={14}/><b>translit</b><span>YOUR STORY, UNDERSTOOD.</span></div><div className="titlebar-drag" data-tauri-drag-region onMouseDown={e=>{if(e.button===0&&isTauri())void getCurrentWindow().startDragging();}} onDoubleClick={()=>{if(isTauri())void getCurrentWindow().toggleMaximize();}}/><div className="window-buttons"><button aria-label="Свернуть окно" onClick={()=>{if(isTauri())void getCurrentWindow().minimize();}}><Minus size={15}/></button><button aria-label="Развернуть окно" onClick={()=>{if(isTauri())void getCurrentWindow().toggleMaximize();}}><Square size={12}/></button><button className="close-window" aria-label="Закрыть приложение" onClick={()=>{if(isTauri())void getCurrentWindow().close();}}><X size={16}/></button></div></div>;
}
