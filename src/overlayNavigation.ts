import type {Frame} from './types';

export type SelectionUnit={start:number;end:number;text:string;x:number;y:number;height:number};

/** Use the same selectable ranges for controller navigation and mouse targets. */
export function selectionUnits(frame:Frame,mode:string,dialogueOnly:boolean):SelectionUnit[]{
 const visible=(index:number)=>!dialogueOnly||frame.ocr.words[index].y>frame.ocr.height*.45;
 const ranges=mode==='blocks'?frame.blocks.filter(b=>visible(b.start)):frame.ocr.words.map((w,i)=>({start:i,end:i,text:w.text})).filter(b=>visible(b.start));
 return ranges.map(b=>{const words=frame.ocr.words.slice(b.start,b.end+1),left=Math.min(...words.map(w=>w.x)),top=Math.min(...words.map(w=>w.y)),bottom=Math.max(...words.map(w=>w.y+w.height));return {...b,x:left,y:top,height:bottom-top};});
}

/** Move by one word or whole phrase, preserving the selected mode. */
export function moveSelection(units:SelectionUnit[],range:[number,number]|null,action:string):SelectionUnit|undefined{
 if(!units.length)return;
 const index=units.findIndex(u=>range&&u.start<=range[0]&&u.end>=range[1]);
 if(index<0)return units[action==='left'?units.length-1:0];
 if(action==='left'||action==='right')return units[(index+(action==='left'?-1:1)+units.length)%units.length];
 const current=units[index];
 return units.filter(u=>action==='up'?u.y<current.y-current.height*.5:u.y>current.y+current.height*.5)
 .sort((a,b)=>(Math.abs(a.x-current.x)+Math.abs(a.y-current.y)*2)-(Math.abs(b.x-current.x)+Math.abs(b.y-current.y)*2))[0]??current;
}
