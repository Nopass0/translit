/** Normalize browser keyboard events to the names accepted by the native binding parser. */
export function eventBinding(e:KeyboardEvent|{key:string;code:string;ctrlKey:boolean;altKey:boolean;shiftKey:boolean;metaKey:boolean}):string{
 if(e.metaKey||['Control','Alt','Shift','Meta'].includes(e.key))return '';
 const names:Record<string,string>={' ':'Space',ArrowLeft:'Left',ArrowRight:'Right',ArrowUp:'Up',ArrowDown:'Down'};
 const key=e.code.startsWith('Numpad')?e.code:e.code.startsWith('Key')?e.code.slice(3):e.code.startsWith('Digit')?e.code.slice(5):names[e.key]??e.key.toUpperCase();
 return [e.ctrlKey?'Ctrl':'',e.altKey?'Alt':'',e.shiftKey?'Shift':'',key].filter(Boolean).join('+');
}
