import {availableMonitors,getCurrentWindow} from '@tauri-apps/api/window';
import {isTauri} from '@tauri-apps/api/core';

export type VisibleBounds={left:number;top:number;width:number;height:number};

/** Find the largest visible monitor intersection, expressed in webview coordinates. */
export async function visibleBounds():Promise<VisibleBounds>{
 const fallback={left:0,top:0,width:window.innerWidth,height:window.innerHeight};
 if(!isTauri())return fallback;
 const current=getCurrentWindow();
 const [monitors,position,scale]=await Promise.all([availableMonitors(),current.outerPosition(),current.scaleFactor()]);
 const intersections=monitors.map(m=>{
  const left=Math.max(0,(m.position.x-position.x)/scale),top=Math.max(0,(m.position.y-position.y)/scale);
  const right=Math.min(window.innerWidth,(m.position.x+m.size.width-position.x)/scale);
  const bottom=Math.min(window.innerHeight,(m.position.y+m.size.height-position.y)/scale);
  return {left,top,width:Math.max(0,right-left),height:Math.max(0,bottom-top)};
 }).filter(b=>b.width>0&&b.height>0).sort((a,b)=>b.width*b.height-a.width*a.height);
 return intersections[0]??fallback;
}

/** Keep the complete popup inside a visible monitor even when the game extends off screen. */
export function clampPopup(position:{x:number;y:number},width:number,height:number,bounds:VisibleBounds){
 const xMargin=Math.min(8,Math.max(0,(bounds.width-width)/2)),yMargin=Math.min(8,Math.max(0,(bounds.height-height)/2));
 return {x:Math.max(bounds.left+xMargin,Math.min(position.x,bounds.left+bounds.width-width-xMargin)),y:Math.max(bounds.top+yMargin,Math.min(position.y,bounds.top+bounds.height-height-yMargin))};
}
