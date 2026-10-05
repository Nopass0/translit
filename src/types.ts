export type Translator = {provider:string;endpoint:string;model:string;target_language:string;history_lines:number;mini_model:string;threads:number;device:string;preload:boolean;auto_install:boolean};
export type DecisionSettings={enabled:boolean;preload:boolean;auto_install:boolean;threads:number;tokens:number};
export type SubtitleSettings={enabled:boolean;interval_ms:number;region_top:number;region_height:number;font_size:number};
export type Settings = {hotkey:string;gamepad:string;auto_pause:boolean;dialogue_only:boolean;overlay:boolean;capture_backend:string;translator:Translator;decision:DecisionSettings;subtitles:SubtitleSettings};
export type Entry = {word:string;translation:string;context:string;game:string;created_at:number;analysis?:ContextTranslation|null;review_count?:number;learned?:boolean;query_count?:number;due_at?:number;interval_days?:number;streak?:number;lapses?:number;last_reviewed?:number};
export type Game = {pid:number;name:string;title:string;path:string;hwnd:number;api:string;x64:boolean;x:number;y:number;width:number;height:number};
export type Status = {game:Game|null;paused:boolean;busy:boolean;settings:Settings;entries:Entry[];dictionary_size:number;capture_mode:string};
export type Word = {text:string;line:number;x:number;y:number;width:number;height:number};
export type Block = {text:string;start:number;end:number;kind:string};
export type Frame = {image:string;ocr:{width:number;height:number;text:string;words:Word[]};paused:boolean;warning:string|null;history:string[];game_title:string;blocks:Block[]};
export type DecisionAnswer={choice:string;confidence:number;probabilities:number[];truncated:boolean};
export type DecisionAnalysis={answers:Record<string,DecisionAnswer>;elapsed_ms:number};
export type ContextTranslation = {selection:string;translation:string;context_translation:string;phrase:string;explanation:string;construction:string;alternatives:string[];source:string;idiom:boolean;grammar?:string;examples?:string[];tokens?:GrammarToken[];situation?:string;grammar_source?:string;query_count?:number;decisions?:{candidates:string[];analysis:DecisionAnalysis}|null};
export const defaults:Status = {game:null,paused:false,busy:false,settings:{hotkey:'F8',gamepad:'shoulders',auto_pause:true,dialogue_only:true,overlay:true,capture_backend:'auto',translator:{provider:'mini',endpoint:'http://127.0.0.1:11434',model:'',target_language:'ru',history_lines:6,mini_model:'opus',threads:0,device:'cpu',preload:true,auto_install:true},decision:{enabled:true,preload:true,auto_install:true,threads:2,tokens:512},subtitles:{enabled:false,interval_ms:500,region_top:0.62,region_height:0.32,font_size:30}},entries:[],dictionary_size:0,capture_mode:'hook'};

export type GrammarToken={text:string;start:number;end:number;lemma:string;pos:string;label:string;role:string;verb_form:string;irregular:boolean;forms:string[];dependency:string};
export type GrammarAnalysis={tokens:GrammarToken[];source:string};
