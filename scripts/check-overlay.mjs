/** Regression checks for phrase navigation and popup bounds without a running game. */
import {readFileSync} from 'node:fs';
import {strict as assert} from 'node:assert';
import ts from 'typescript';
import vm from 'node:vm';
function module(path){
 const source=readFileSync(path,'utf8').replace(/^import .*;$/gm,'');
 const code=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 const exports={};vm.runInNewContext(code,{exports});return exports;
}
const {selectionUnits,moveSelection}=module('src/overlayNavigation.ts');
const {clampPopup}=module('src/overlayBounds.ts');
const {eventBinding}=module('src/keyBinding.ts');
assert.equal(eventBinding({key:'й',code:'KeyQ',ctrlKey:true,altKey:false,shiftKey:false,metaKey:false}),'Ctrl+Q');
assert.equal(eventBinding({key:'!',code:'Digit1',ctrlKey:false,altKey:true,shiftKey:true,metaKey:false}),'Alt+Shift+1');
assert.equal(eventBinding({key:'0',code:'Numpad0',ctrlKey:false,altKey:false,shiftKey:false,metaKey:false}),'Numpad0');
const words=['Stop','trying','to','butter','me','up.','Please','listen.'].map((text,i)=>({text,line:i<6?0:1,x:(i%6)*40,y:i<6?600:640,width:35,height:20}));
const frame={ocr:{words,height:720},blocks:[{start:0,end:2,text:'Stop trying to'},{start:3,end:5,text:'butter me up.'},{start:6,end:7,text:'Please listen.'}]};
const blocks=selectionUnits(frame,'blocks',true);
assert.equal(blocks.length,3);assert.equal(moveSelection(blocks,null,'right').text,'Stop trying to');
assert.equal(moveSelection(blocks,[0,2],'right').text,'butter me up.');
assert.equal(moveSelection(blocks,[3,5],'right').text,'Please listen.');
assert.equal(moveSelection(blocks,[6,7],'right').text,'Stop trying to');
assert.equal(moveSelection(blocks,[3,5],'left').end,2);
assert.equal(selectionUnits(frame,'words',true).length,8);
assert.equal(moveSelection(selectionUnits(frame,'words',true),[3,3],'right').text,'me');
for(const bounds of [{left:0,top:0,width:3440,height:1440},{left:500,top:50,width:320,height:240},{left:0,top:0,width:12,height:12}]){
 const w=Math.max(1,Math.min(640,bounds.width-16)),h=Math.max(1,Math.min(850,bounds.height-16));
 for(const p of [{x:-1000,y:-1000},{x:9999,y:9999}]){const result=clampPopup(p,w,h,bounds);assert(result.x>=bounds.left);assert(result.y>=bounds.top);assert(result.x+w<=bounds.left+bounds.width);assert(result.y+h<=bounds.top+bounds.height);}
}
console.log('PASS: phrase/word navigation, wraparound, small/off-screen monitor popup bounds');
