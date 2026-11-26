import {useState} from 'react';
import {Button} from '@rintawa/ui';
export type Affect = {revision:string;model:string;enabled:boolean;valence:number;activation:number;stress:number;elapsedSeconds:string;appliedCount:string;lastRecordId:string|null};
export type Deltas = {valence:number;activation:number;stress:number};
export type Appraisal = {model:string;recordRevision:string;elapsedSeconds:number;deltas:Deltas;resultingStateRevision:string};
const secondsValid=(v:string)=>v.trim()!==''&&Number.isInteger(Number(v))&&Number(v)>=0&&Number(v)<=86400;
export function AffectControls({state,disabled,onToggle,onAdvance}:{state:Affect|null;disabled:boolean;onToggle:()=>void;onAdvance:(seconds:number)=>void}){
 const [seconds,setSeconds]=useState('600');
 return <section aria-label="Fictional affect"><h2>Psychology · affect-v1</h2><p>Fictional authored model. Reads do not advance time. Only explicitly appraised records contribute.</p>
 <small>Valence −100…100 · activation/stress 0…100. One point decays toward zero per 600 simulation seconds.</small>
 {state&&<><p className="affect-values">Valence {state.valence} · Activation {state.activation} · Stress {state.stress}</p><small>State revision {state.revision} · simulation seconds {state.elapsedSeconds} · appraisals {state.appliedCount}</small></>}
 <div className="actions"><Button disabled={disabled} onClick={onToggle}>{state?.enabled?'Disable affect':'Enable affect'}</Button></div>
 <small>If AI memory context is allowed for this subscription, enabled affect is also shared with Narrator.</small>
 <label>Advance simulation seconds<input aria-label="Affect elapsed seconds" type="number" min={0} max={86400} step={1} value={seconds} disabled={disabled||!state?.enabled} onChange={e=>setSeconds(e.target.value)}/></label>
 <Button disabled={disabled||!state?.enabled||!secondsValid(seconds)} onClick={()=>onAdvance(Number(seconds))}>Advance affect time</Button></section>;
}
export function AppraisalControls({state,appraisal,recordRevision,disabled,onAppraise}:{state:Affect|null;appraisal:Appraisal|null;recordRevision:string;disabled:boolean;onAppraise:(seconds:number,deltas:Deltas)=>void}){
 const [values,setValues]=useState({valence:'0',activation:'0',stress:'0'});const [seconds,setSeconds]=useState('0');
 const valid=secondsValid(seconds)&&Object.values(values).every(v=>v.trim()!==''&&Number.isInteger(Number(v))&&Number(v)>=-20&&Number(v)<=20);
 if(appraisal)return <section aria-label="Applied appraisal"><h3>Appraisal applied</h3><p>Captured record revision {appraisal.recordRevision} · resulting state revision {appraisal.resultingStateRevision}</p><small>{JSON.stringify(appraisal.deltas)} · {appraisal.elapsedSeconds} seconds. Editing this record does not replay or undo that appraisal.{recordRevision!==appraisal.recordRevision?' The record has changed since appraisal.':''}</small></section>;
 return <section aria-label="Appraise recollection"><h3>Appraise this recollection</h3><small>Subjective authored deltas, once per record. Decay applies before these deltas. A new authored recollection may express a later correction.</small>
 {(['valence','activation','stress'] as const).map(key=><label key={key}>{key} delta<input aria-label={`Appraisal ${key}`} type="number" min={-20} max={20} step={1} value={values[key]} disabled={disabled||!state?.enabled} onChange={e=>setValues({...values,[key]:e.target.value})}/></label>)}
 <label>Elapsed simulation seconds<input aria-label="Appraisal elapsed seconds" type="number" min={0} max={86400} step={1} value={seconds} disabled={disabled||!state?.enabled} onChange={e=>setSeconds(e.target.value)}/></label>
 <Button disabled={disabled||!state?.enabled||!valid} onClick={()=>onAppraise(Number(seconds),{valence:Number(values.valence),activation:Number(values.activation),stress:Number(values.stress)})}>Apply appraisal once</Button></section>;
}
