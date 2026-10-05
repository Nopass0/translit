import type {Settings} from './types';
export function SubtitlePanel({settings,onChange}:{settings:Settings;onChange:(patch:Partial<Settings>)=>Promise<void>}){
 const live=settings.subtitles;
 function update(patch:Partial<Settings['subtitles']>){void onChange({subtitles:{...live,...patch}});}
 return <section className="settings-card full subtitle-settings"><h2>Русские субтитры в игре</h2><p>Снимает только выбранную область, объединяет переносы строк и переводит новую реплику после проверки OCR. Игра продолжает идти. Окно клики не перехватывает.</p>
  <label className="setting-toggle"><div><strong>Показывать перевод над субтитрами</strong><p>Использует быстрый локальный перевод OPUS и не ставит игру на паузу.</p></div><input type="checkbox" checked={live.enabled} onChange={e=>update({enabled:e.target.checked})}/></label>
  <label className="field-label">Частота проверки<select value={live.interval_ms} onChange={e=>update({interval_ms:Number(e.target.value)})}>{[300,400,500,750,1000].map(ms=><option key={ms} value={ms}>{ms} мс · {Math.round(1000/ms)} кадр/с</option>)}</select></label>
  <label className="field-label">Область оригинальных субтитров<select value={live.region_top} onChange={e=>update({region_top:Number(e.target.value)})}>{[.48,.55,.62,.68,.74].map(v=><option key={v} value={v}>{Math.round(v*100)}% высоты экрана</option>)}</select></label>
  <label className="field-label">Высота области OCR<select value={live.region_height} onChange={e=>update({region_height:Number(e.target.value)})}>{[.15,.22,.32,.40,.45].map(v=><option key={v} value={v}>{Math.round(v*100)}% экрана</option>)}</select></label>
  <label className="field-label">Размер русского текста<select value={live.font_size} onChange={e=>update({font_size:Number(e.target.value)})}>{[24,30,36,42,48].map(v=><option key={v} value={v}>{v} px</option>)}</select></label>
 </section>;
}
