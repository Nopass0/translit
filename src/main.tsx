import React, { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { BookOpen, ScanText, Settings2, Gamepad2, Keyboard, Play, Pause, Plus, ArrowUpRight, RefreshCw, Search, Check, X, Link2, Unplug, Download, Trash2, ChevronRight, Monitor, Sparkles, CircleHelp } from 'lucide-react';
import './styles.css';
import {Titlebar} from './Titlebar';
import {TranslatorPanel} from './TranslatorPanel';
import {DecisionPanel} from './DecisionPanel';
import {LiveCaptions} from './LiveCaptions';
import {SubtitlePanel} from './SubtitlePanel';
import {Overlay} from './Overlay';
import {Study} from './Study';
import {GrammarView} from './GrammarView';
import {eventBinding} from './keyBinding';
import {defaults} from './types';
import type {Settings,Entry,Game,Status,Word,Frame,ContextTranslation} from './types';

type Definition = {query:string;lemma:string;translations:string[];source:string};
const empty = defaults;
const controllerNames:Record<string,string> = {shoulders:'LB + RB',back:'View / Back',start:'Menu / Start',ls:'Левый стик (L3)',rs:'Правый стик (R3)',off:'Отключён',LB:'LB',RB:'RB',LT:'LT',RT:'RT',A:'A',B:'B',X:'X',Y:'Y','LT+RT':'LT + RT','Back+Start':'View + Menu','LB+A':'LB + A','RB+A':'RB + A','LT+RB':'LT + RB','L3+R3':'L3 + R3',Up:'D-pad ↑',Down:'D-pad ↓',Left:'D-pad ←',Right:'D-pad →'};

function App(){
  const [status,setStatus]=useState<Status>(empty);
  const [games,setGames]=useState<Game[]>([]);
  const [tab,setTab]=useState<'translate'|'dictionary'|'settings'|'study'>('translate');
  const [frame,setFrame]=useState<Frame|null>(null);
  const [definition,setDefinition]=useState<Definition|null>(null);
  const [selected,setSelected]=useState<Word|null>(null);
  const [query,setQuery]=useState('');
  const [gameFilter,setGameFilter]=useState('');
  const [contextInfo,setContextInfo]=useState<ContextTranslation|null>(null);
  const [contextBusy,setContextBusy]=useState(false);
  const [translation,setTranslation]=useState('');
  const [context,setContext]=useState('');
  const [filter,setFilter]=useState('');
  const [busy,setBusy]=useState(false);
  const [progress,setProgress]=useState('');
  const [toast,setToast]=useState<{text:string;error:boolean}|null>(null);
  const [saved,setSaved]=useState(false);
  const lookupVersion=useRef(0);
  const contentRef=useRef<HTMLElement>(null);
  useEffect(()=>{contentRef.current?.scrollTo({top:0,behavior:"instant"});},[tab]);
  const stateRef=useRef({status,frame,translation,definition,context,selected,contextInfo,contextBusy,tab});
  stateRef.current={status,frame,translation,definition,context,selected,contextInfo,contextBusy,tab};
  const native=isTauri();

  function message(text:string,error=false){setToast({text,error});}
  async function refresh(){if(native)setStatus(await invoke<Status>('get_status'));}
  async function refreshGames(){if(native)setGames(await invoke<Game[]>('list_games'));}
  async function action<T>(job:()=>Promise<T>,rethrow=false):Promise<T|undefined>{setBusy(true);try{return await job();}catch(e){message(String(e),true);if(rethrow)throw e;}finally{setBusy(false);}}
  async function attach(game:Game){await action(async()=>{await invoke('attach_game',{pid:game.pid});await refresh();message('Игра подключена. Можно получить кадр.');});}
  function acceptFrame(next:Frame){setContextInfo(null);setFrame(next);setTab('translate');setSelected(null);setDefinition(null);setTranslation('');setSaved(false);setProgress('');if(next.warning)message(next.warning,true);}
  async function capture(){await action(async()=>{setProgress('Получаю кадр игры…');try{acceptFrame(await invoke<Frame>('capture_frame'));}finally{setProgress('');await refresh();}});}
  async function resume(){await action(async()=>{await invoke('resume_game');await refresh();});}
  async function detach(){await action(async()=>{await invoke('detach_game');await refresh();setFrame(null);message('Перехват рендера отключён');});}
  async function lookup(word:string,box:Word|null=null){
    const frame=stateRef.current.frame,status=stateRef.current.status,context=stateRef.current.context;
    const version=++lookupVersion.current;setContextInfo(null);setContextBusy(false);setQuery(word);setSelected(box);setSaved(false);setDefinition(null);setTranslation('');
    if(box && frame)setContext(box.y>frame.ocr.height*.45?(frame.history.at(-1)??frame.ocr.text):frame.ocr.words.filter(w=>w.line===box.line).map(w=>w.text).join(' '));
    const sentence=box&&frame?(box.y>frame.ocr.height*.45?(frame.history.at(-1)??frame.ocr.text):frame.ocr.words.filter(w=>w.line===box.line).map(w=>w.text).join(' ')):context;
    try{const result=await invoke<Definition>('lookup_word',{word});if(version===lookupVersion.current){setDefinition(result);setTranslation(result.translations[0]??'');}
      const evidence=await invoke<ContextTranslation>('local_translation',{selection:word,context:sentence});
      if(version===lookupVersion.current){setContextInfo(evidence);if(evidence.translation)setTranslation(evidence.translation);}
      if(version===lookupVersion.current){setContextBusy(true);const meaning=await invoke<ContextTranslation>('translate_selection',{selection:word,context:sentence});if(version===lookupVersion.current){setContextInfo(meaning);setTranslation(meaning.translation);}}
    }catch(e){message(String(e),true);}finally{if(version===lookupVersion.current)setContextBusy(false);}
  }
  async function save(){const current=stateRef.current;if(!current.definition||current.contextBusy)return;await action(async()=>{await invoke('save_entry',{entry:{word:current.contextInfo?.idiom?current.contextInfo.phrase:current.definition!.query,translation:current.translation,context:current.context,game:current.status.game?.title??current.status.entries.find(entry=>entry.word===(current.contextInfo?.idiom?current.contextInfo.phrase:current.definition!.query))?.game??'',created_at:0,analysis:current.contextInfo}});await refresh();setSaved(true);message('Слово сохранено в личный словарь');});}
  async function settings(patch:Partial<Settings>){await action(async()=>{await invoke('save_settings',{settings:{...status.settings,...patch}});if(patch.capture_backend&&patch.capture_backend!==status.settings.capture_backend&&status.game){await invoke('attach_game',{pid:status.game.pid});setFrame(null);}await refresh();},true);}
  useEffect(()=>{
    if(!native)return;
    void action(async()=>{await refresh();await refreshGames();});
    const listeners=[listen<Frame>('captured',e=>{acceptFrame(e.payload);void refresh();}),listen<string>('app-error',e=>{message(e.payload,true);setProgress('');void refresh();}),listen<string>('capture-progress',e=>setProgress(e.payload)),listen('status-changed',()=>void refresh())];
    const interval=setInterval(()=>void refresh().catch(e=>message(String(e),true)),2000);
    return()=>{clearInterval(interval);listeners.forEach(p=>void p.then(unlisten=>unlisten()));};
  },[]);
  useEffect(()=>{if(toast){const id=setTimeout(()=>setToast(null),6000);return()=>clearTimeout(id);}},[toast]);
  useEffect(()=>{
    const keyboard=(e:KeyboardEvent)=>{if(stateRef.current.status.paused&&(e.key==='Escape'||(!(e.target as HTMLElement)?.matches('input,textarea')&&eventBinding(e).toLowerCase()===stateRef.current.status.settings.hotkey.toLowerCase()))){e.preventDefault();void resume();}if(e.ctrlKey&&e.key==='Enter'){e.preventDefault();void save();}};
    window.addEventListener('keydown',keyboard);return()=>window.removeEventListener('keydown',keyboard);
  },[]);
  useEffect(()=>{
    if(!native)return;
    let index=-1;
    const sub=listen<string>('controller-action',e=>{
      const current=stateRef.current;
      if(current.tab==='study')return;
      if(e.payload==='resume'&&current.status.paused){void resume();return;}
      if(e.payload==='save'){void save();return;}
      if(e.payload==='mode'){setTab('study');return;}
      const words=current.frame?.ocr.words.filter(w=>!current.status.settings.dialogue_only||w.y>current.frame!.ocr.height*.45)??[];
      if(!words.length)return;
      if(['left','right','up','down'].includes(e.payload)){
        index=(index+(['left','up'].includes(e.payload)?-1:1)+words.length)%words.length;
        setTab('translate');setSelected(words[index]);setQuery(words[index].text);
      }
      if(e.payload==='choose'){index=Math.min(Math.max(0,index),words.length-1);void lookup(words[index].text,words[index]);}
    });return()=>{void sub.then(f=>f());};
  },[]);
  // Controller navigation is active in the focused dictionary window.
  useEffect(()=>{
    if(native)return;let id=0;let previous:boolean[]=[];let wordIndex=-1;
    function tick(){
      const pad=Array.from(navigator.getGamepads?.()??[]).find(Boolean);
      if(pad && document.hasFocus() && stateRef.current.status.paused){
        const buttons=pad.buttons.map(b=>b.pressed);
        const edge=(i:number)=>buttons[i]&&!previous[i];
        const current=stateRef.current;
        const words=current.frame?.ocr.words.filter(w=>!current.status.settings.dialogue_only||w.y>(current.frame!.ocr.height*.45))??[];
        if((edge(14)||edge(15))&&words.length){wordIndex=(wordIndex+(edge(14)?-1:1)+words.length)%words.length;void lookup(words[wordIndex].text,words[wordIndex]);}
        if(edge(0)&&words.length){wordIndex=Math.max(0,wordIndex);void lookup(words[wordIndex].text,words[wordIndex]);}
        if(edge(2)&&current.definition&&current.translation)void save();
        if(edge(1))void resume();
        previous=buttons;
      }else previous=[];
      id=requestAnimationFrame(tick);
    }
    tick();return()=>cancelAnimationFrame(id);
  },[frame]);

  const visibleWords=frame?.ocr.words.filter(w=>!status.settings.dialogue_only||w.y>frame.ocr.height*.45)??[];
  const entries=status.entries.filter(e=>(e.word+' '+e.translation).toLowerCase().includes(filter.toLowerCase()));
  const working=busy||status.busy||Boolean(progress);
  return <div className="app"><Titlebar/>
    <aside className="sidebar">
      <div className="brand"><span className="brand-mark"><BookOpen size={23}/></span><div>translit<span>PLAY. READ. REMEMBER.</span></div></div>
      <div className="nav-label">ВАШЕ ПРОСТРАНСТВО</div>
      <nav>
        <button className={tab==='translate'?'active':''} onClick={()=>setTab('translate')}><ScanText size={19}/>Перевод<span className="nav-dot"/></button>
        <button className={tab==='dictionary'?'active':''} onClick={()=>setTab('dictionary')}><BookOpen size={19}/>Мой словарь<span className="count">{status.entries.length}</span></button>
        <button className={tab==='study'?'active':''} onClick={()=>setTab('study')}><BookOpen size={19}/>Изучение</button><button className={tab==='settings'?'active':''} onClick={()=>setTab('settings')}><Settings2 size={19}/>Настройки</button>
      </nav>
      <div className="sidebar-game">
        <div className="game-info"><span className="eyebrow">ИГРОВОЙ ПРОФИЛЬ</span><h3>{status.game?.title??"Выберите свою игру"}</h3><p><span className={'dot '+(status.game?'green':'')}/>{status.game?(status.capture_mode==='hook'?'DirectX 11 · DLL подключена':'Захват игрового окна'):'Ожидание подключения'}</p></div>
      </div>
      <div className="sidebar-bottom"><span className="language">EN <ChevronRight size={12}/> RU</span><span>Локальный словарь</span></div>
    </aside>
    <main ref={contentRef}>
      <header><div className="breadcrumb">Библиотека <ChevronRight size={14}/><span>{tab==='translate'?'Перевод в игре':tab==='dictionary'?'Личный словарь':tab==='study'?'Изучение':'Настройки'}</span></div><div className={'status-pill '+(status.paused?'paused':'')}><span className={'dot '+(status.game?'green':'')}/>{status.paused?'Игра на паузе':status.game?'Готова к переводу':'Игра не подключена'}</div></header>
      <div className="page">
        <div className="page-heading"><div><span className="eyebrow mint">{tab==='translate'?'ЯЗЫК ОТКРЫВАЕТ НОВЫЕ МИРЫ':tab==='dictionary'?'ЗАПОМНИТЬ, ЧТОБЫ ПОНЯТЬ':tab==='study'?'УЧИТЕСЬ НА СВОИХ ИСТОРИЯХ':'ПОД ВАШ СТИЛЬ ИГРЫ'}</span><h1>{tab==='translate'?'Каждое слово — часть истории.':tab==='dictionary'?'Ваш словарь приключений.':tab==='study'?'Вспомните. Повторите. Используйте.':'Один жест до перевода.'}</h1><p>{tab==='translate'?'Остановите момент. Выберите слово. Продолжайте путешествие.':tab==='dictionary'?'Слова, которые вы встретили в игре, вместе с их контекстом.':tab==='study'?'Карточки с контекстом, грамматикой и примерами из вашего словаря.':'Назначьте удобные кнопки и настройте чтение игрового текста.'}</p></div><span className="local-badge"><span className="dot green"/>OFFLINE FIRST</span></div>
        {!native&&<div className="notice">Это предпросмотр интерфейса. Для подключения к игре запустите приложение через <code>npm run start</code>.</div>}
        {tab==='translate'&&<>
          <div className="session-bar"><div className="session-title"><Monitor size={20}/><div><strong>{status.game?.title??'Подключите запущенную игру'}</strong><span>{status.game?`PID ${status.game.pid} · ${status.capture_mode==='hook'?'DirectX 11 DLL':'Захват окна'} · английский → русский`:'DirectX 11 · Vulkan / DX12 / DX9 через захват окна'}</span></div></div><div className="session-actions">{frame&&<button className="secondary" disabled={working} onClick={()=>void action(async()=>{await invoke("show_overlay");})}><ScanText size={16}/>Оверлей</button>}{status.game?<><button className="icon-button" title="Отключить DLL" disabled={working} onClick={()=>void detach()}><Unplug size={18}/></button><button className="primary" disabled={working} onClick={()=>void(status.paused?resume():capture())}>{status.paused?<Play size={16}/>:<ScanText size={16}/>} {status.paused?'Продолжить игру':'Получить кадр'}<kbd>{status.settings.hotkey}</kbd></button></>:<button className="secondary" disabled={!native||working} onClick={()=>void action(refreshGames)}><RefreshCw size={16}/>Найти игру</button>}</div></div>
          {!status.game&&<div className="connect-panel"><Link2 size={22}/><div><h3>Подключите окно игры</h3><p>Выберите окно игры. Режим захвата определяется автоматически; его можно изменить в настройках.</p><div className="word-search game-filter"><Search size={15}/><input placeholder="Название игры или процесса…" value={gameFilter} onChange={e=>setGameFilter(e.target.value)}/></div>{games.length?games.filter(g=>(g.title+g.name).toLowerCase().includes(gameFilter.toLowerCase())).sort((a,b)=>Number(b.api==="dx11")-Number(a.api==="dx11")).map(g=><button key={g.pid} className="connect-game" disabled={working} onClick={()=>void attach(g)}><Gamepad2 size={17}/>{g.title}<span>{g.api==="dx11"&&g.x64?"DX11 DLL":"Захват окна"} · PID {g.pid}</span><ArrowUpRight size={15}/></button>):<small>Окно игры пока не найдено. После запуска нажмите «Найти игру».</small>}</div></div>}
          <div className="workspace">
            <section className="frame-panel"><div className="panel-heading"><span><ScanText size={16}/>Игровой кадр</span><label className="compact-switch"><input type="checkbox" checked={status.settings.dialogue_only} disabled={!native||working} onChange={e=>void settings({dialogue_only:e.target.checked})}/>Только диалоги</label></div>
              <div className={'frame-stage '+(frame?'has-frame':'')}>
                {frame?<div className="captured-image"><img src={frame.image} alt="Кадр, полученный из DirectX 11 игры"/>{visibleWords.map((w,i)=><button key={i} title={`Перевести «${w.text}»`} aria-label={`Перевести ${w.text}`} className={'word-box '+(selected===w?'selected':'')} style={{left:`${w.x/frame.ocr.width*100}%`,top:`${w.y/frame.ocr.height*100}%`,width:`${w.width/frame.ocr.width*100}%`,height:`${w.height/frame.ocr.height*100}%`}} onClick={()=>void lookup(w.text,w)}/>)}</div>:<div className="empty-frame"><div className="scan-illustration"><ScanText size={42}/><span className="scan-corner tl"/><span className="scan-corner br"/></div><h2>История ждёт своего перевода</h2><p>Откройте диалог в игре и нажмите<br/><kbd>{status.settings.hotkey}</kbd> или <kbd>{controllerNames[status.settings.gamepad]}</kbd></p><span className="tiny-tag">Кадр появится здесь</span></div>}
                {progress&&<div className="progress-overlay"><RefreshCw className="spin" size={28}/><strong>{progress}</strong></div>}
              </div>
              <div className="frame-caption"><span className="dot mint-dot"/>{frame?`${visibleWords.length} слов · нажмите на слово в кадре`: 'Кадр и распознавание текста обрабатываются на вашем компьютере'}<span className="frame-format">{frame?`${frame.ocr.width||'—'} × ${frame.ocr.height||'—'}`:'DX11'}</span></div>
              {frame&&<div className="recognized"><span className="eyebrow">РАСПОЗНАННЫЙ ТЕКСТ</span><div>{visibleWords.length?visibleWords.map((w,i)=><button key={i} className={selected===w?'selected-token':''} onClick={()=>void lookup(w.text,w)}>{w.text}</button>):<p>Слова не найдены. Покажите весь кадр или введите слово вручную.</p>}</div></div>}
            </section>
            <section className="word-panel"><div className="panel-heading"><span><BookOpen size={16}/>Словарная карточка</span><span className="language">EN → RU</span></div>
              <form className="word-search" onSubmit={e=>{e.preventDefault();if(native&&query.trim())void lookup(query);}}><Search size={17}/><input aria-label="Найти слово" placeholder="Введите или выберите слово" value={query} onChange={e=>setQuery(e.target.value)}/><button disabled={!native||!query.trim()} title="Искать"><ChevronRight size={18}/></button></form>
              {definition?<div className="definition"><span className="eyebrow">{definition.source}</span><h2>{definition.query}</h2>{definition.query!==definition.lemma&&<p className="lemma">Начальная форма: {definition.lemma}</p>}<div className="translation-options">{definition.translations.slice(0,10).map((t,i)=><button key={i} className={translation===t?'chosen':''} onClick={()=>setTranslation(t)}><span>{String(i+1).padStart(2,'0')}</span>{t}</button>)}</div>{contextBusy&&<p className="context-pending"><RefreshCw size={13} className="spin"/>Уточняю смысл в контексте…</p>}{contextInfo&&<div className="context-meaning"><span className="eyebrow">{contextInfo.construction}</span>{contextInfo.idiom&&<h3>{contextInfo.phrase}</h3>}<p>{contextInfo.explanation}</p><GrammarView analysis={contextInfo}/>{contextInfo.context_translation&&<blockquote>{contextInfo.context_translation}</blockquote>}<div className="grammar-panel">{contextInfo.grammar&&<><h3>Грамматика для изучения</h3><p>{contextInfo.grammar}</p></>}{Boolean(contextInfo.examples?.length)&&<><h3>Примеры</h3>{contextInfo.examples?.map((example,i)=><p key={i}>{example}</p>)}</>}</div><small>{contextInfo.source}</small></div>}<label className="field-label">Ваш перевод<textarea placeholder="Введите перевод, если статья не найдена" value={translation} onChange={e=>{setTranslation(e.target.value);setSaved(false);}} rows={3}/></label><label className="field-label">Контекст из игры<textarea value={context} onChange={e=>setContext(e.target.value)} placeholder="Фраза, в которой встретилось слово" rows={3}/></label><button className={'primary save-word '+(saved?'saved':'')} disabled={working||contextBusy||!translation.trim()} onClick={()=>void save()}>{saved?<Check size={17}/>:<Plus size={17}/>} {saved?'В вашем словаре':'Добавить в словарь'}<kbd>Ctrl ↵</kbd></button></div>:<div className="empty-word"><div className="word-orbit"><BookOpen size={31}/><Sparkles size={16}/></div><h3>У каждого слова<br/>есть история</h3><p>Выберите слово на игровом кадре<br/>или введите его в поиск.</p><div className="dictionary-stat"><strong>{status.dictionary_size.toLocaleString('ru-RU')}</strong><span>статей в словаре FreeDict</span></div></div>}
            </section>
          </div>
          <div className="shortcut-strip"><span><Keyboard size={17}/><kbd>{status.settings.hotkey}</kbd>Кадр / продолжить</span><span><Gamepad2 size={17}/><kbd>{controllerNames[status.settings.gamepad]}</kbd>Вызвать перевод</span><span><kbd>← →</kbd>Слово <kbd>A</kbd>Выбрать <kbd>X</kbd>Сохранить <kbd>B</kbd>В игру</span><CircleHelp size={17}/></div>
        </>}
        {tab==='dictionary'&&<section className="dictionary-panel"><div className="dictionary-toolbar"><div className="word-search"><Search size={18}/><input placeholder="Поиск в ваших словах…" aria-label="Поиск в словаре" value={filter} onChange={e=>setFilter(e.target.value)}/></div><span>{status.entries.length} слов</span><button className="secondary" disabled={!native||!status.entries.length} onClick={()=>void action(async()=>message('Экспорт: '+await invoke<string>('export_dictionary')))}><Download size={16}/>Экспорт JSON</button></div>{entries.length?<div className="entries">{entries.map(e=><article className="entry" key={e.word}><div><h2>{e.word}</h2><p>{e.translation}</p>{e.context&&<blockquote>{e.context}</blockquote>}{e.analysis&&<details className="saved-analysis"><summary>{e.analysis.construction||"Разбор и примеры"}</summary><p>{e.analysis.explanation}</p><GrammarView analysis={e.analysis}/>{e.analysis.grammar&&<p>{e.analysis.grammar}</p>}{e.analysis.context_translation&&<blockquote>{e.analysis.context_translation}</blockquote>}{e.analysis.examples?.map((example,i)=><p key={i}>{example}</p>)}</details>}<small>{e.game} · {new Date(e.created_at*1000).toLocaleDateString('ru-RU')}</small></div><div><button className="icon-button" title="Редактировать слово" onClick={()=>{setTab('translate');setQuery(e.word);setDefinition({query:e.word,lemma:e.word,translations:[e.translation],source:'Личный словарь'});setTranslation(e.translation);setContext(e.context);setContextInfo(e.analysis??null);}}><ArrowUpRight size={17}/></button><button className="icon-button delete" title={`Удалить ${e.word}`} disabled={working} onClick={()=>void action(async()=>{await invoke('delete_entry',{word:e.word});await refresh();})}><Trash2 size={17}/></button></div></article>)}</div>:<div className="empty-dictionary"><BookOpen size={46}/><h2>{filter?'Ничего не найдено':'Первое слово начнёт коллекцию'}</h2><p>Сохраняйте слова из игровых диалогов.<br/>Они будут ждать вас здесь — вместе с контекстом.</p><button className="secondary" onClick={()=>setTab('translate')}>Перейти к переводу<ChevronRight size={16}/></button></div>}</section>}
        {tab==='study'&&<Study entries={status.entries} paused={status.paused} onRefresh={refresh} onMessage={message} onBack={()=>setTab("translate")}/>} {tab==='settings'&&<div className="settings-grid"><TranslatorPanel settings={status.settings} onChange={settings} onMessage={message}/><DecisionPanel settings={status.settings.decision} onChange={patch=>settings(patch)} onMessage={message}/><SubtitlePanel settings={status.settings} onChange={settings}/><section className="settings-card"><Keyboard size={24}/><h2>Кнопки вызова</h2><p>Нажмите один раз для захвата. Ещё раз — чтобы продолжить игру.</p><label className="field-label">Клавиатура<select disabled={!native||working} value={status.settings.hotkey} onChange={e=>void settings({hotkey:e.target.value})}>{Array.from(new Set([status.settings.hotkey,...Array.from({length:24},(_,i)=>`F${i+1}`),'Ctrl+Space','Alt+Q','Ctrl+Shift+T','Insert','Home','Numpad0'])).map(key=><option key={key}>{key}</option>)}</select></label><label className="field-label">Своё сочетание<input readOnly aria-label="Назначить сочетание клавиш" placeholder="Нажмите клавишу или сочетание" value={status.settings.hotkey} disabled={!native||working} onKeyDown={e=>{e.preventDefault();e.stopPropagation();const binding=eventBinding(e);if(binding)void settings({hotkey:binding});}}/></label><label className="field-label">Геймпад Xbox / XInput<select disabled={!native||working} value={status.settings.gamepad} onChange={e=>void settings({gamepad:e.target.value})}>{Object.entries(controllerNames).map(([key,name])=><option value={key} key={key}>{name}</option>)}</select></label><p className="settings-note">Центральная Xbox-кнопка зарезервирована Windows / Game Bar и не входит в обычный XInput. Используйте LB+RB или переназначьте её на выбранную клавишу средствами контроллера.</p></section><section className="settings-card"><Pause size={24}/><h2>Чтение без спешки</h2><label className="setting-toggle"><div><strong>Приостанавливать игру</strong><p>Удерживать игровой цикл на кадре; в режиме захвата окна — приостановить процесс.</p></div><input type="checkbox" disabled={!native||working||status.paused} checked={status.settings.auto_pause} onChange={e=>void settings({auto_pause:e.target.checked})}/></label><label className="setting-toggle"><div><strong>Только диалоговая область</strong><p>Выбирать слова в нижних 55% кадра.</p></div><input type="checkbox" disabled={!native||working} checked={status.settings.dialogue_only} onChange={e=>void settings({dialogue_only:e.target.checked})}/></label><div className="recovery-note"><Check size={18}/><p>Отдельный сторож возобновляет игру при закрытии приложения. Пауза также снимается автоматически через 15 минут.</p></div></section><section className="settings-card full"><label className="setting-toggle"><div><strong>Интерактивный оверлей поверх игры</strong><p>Карточка перевода рядом со словом; работает в оконном режиме / без рамки.</p></div><input type="checkbox" checked={status.settings.overlay} onChange={e=>void settings({overlay:e.target.checked})}/></label><label className="field-label">Способ захвата · применяется при подключении игры<select value={status.settings.capture_backend} onChange={e=>void settings({capture_backend:e.target.value})}><option value="auto">Авто · DX11 DLL или захват окна</option><option value="hook">DLL адаптер DirectX 11 x64</option><option value="screen">Захват видимого окна · любой API</option></select></label><BookOpen size={24}/><h2>Локальный словарь и OCR</h2><p>Английский → русский · FreeDict · {status.dictionary_size.toLocaleString('ru-RU')} статей. Перевод и распознавание работают без отправки кадра в интернет. Windows OCR или встроенный Tesseract распознаёт английский текст без отдельной установки.</p><p>Словарь имеет ограниченное покрытие: имена и игровые термины можно добавить вручную. Найденная начальная форма показывается в карточке.</p><div className="settings-links"><a href="https://freedict.org/" target="_blank" rel="noreferrer">FreeDict<ArrowUpRight size={14}/></a><span>DirectX 11 · Windows x64 · Translit 0.2</span></div></section></div>}
      </div>
    </main>
    {toast&&<div className={'toast '+(toast.error?'error':'')} role="status">{toast.error?<CircleHelp size={18}/>:<Check size={18}/>}<span>{toast.text}</span><button onClick={()=>setToast(null)}><X size={16}/></button></div>}
  </div>;
}
const query=new URLSearchParams(location.search);
createRoot(document.getElementById('root')!).render(query.has('subtitles')?<LiveCaptions/>:query.has('overlay')?<Overlay/>:<App/>);
