import {useState,useEffect} from 'react';
import {Button} from '@rintawa/ui';
export type CycleSettings={decayCycles:number;batchSize:number;interpretation:string};
export type CycleState={revision:string;model:'affect-cycles-v1';enabled:boolean;valence:number;activation:number;stress:number;cycles:string;quietCycles:{valence:number;activation:number;stress:number};settings:CycleSettings};
const defaults:CycleSettings={decayCycles:3,batchSize:8,interpretation:''};
export function CycleControls({state,disabled,onConfigure}:{state:CycleState|null;disabled:boolean;onConfigure:(enabled:boolean,settings:CycleSettings)=>void}){
 const [decay,setDecay]=useState(String(state?.settings.decayCycles??defaults.decayCycles));
 const [batch,setBatch]=useState(String(state?.settings.batchSize??defaults.batchSize));
 const [interpretation,setInterpretation]=useState(state?.settings.interpretation??'');
 const settingsKey=JSON.stringify(state?.settings??defaults);
 useEffect(()=>{const saved=JSON.parse(settingsKey) as CycleSettings;setDecay(String(saved.decayCycles));setBatch(String(saved.batchSize));setInterpretation(saved.interpretation);},[settingsKey]);
 const valid=(v:string,min:number,max:number)=>v.trim()!==''&&Number.isInteger(Number(v))&&Number(v)>=min&&Number(v)<=max;
 const settings={decayCycles:Number(decay),batchSize:Number(batch),interpretation};
 const invalid=!valid(decay,1,100)||!valid(batch,1,8)||new TextEncoder().encode(interpretation).length>4096;
 return <section aria-label="Automatic psychology"><h2>Affect analysis</h2>
 <p>{state?.enabled?'Enabled':'Disabled'} · Analysis runs when you process new messages. It sends that batch, the observer name and current psychological state to your configured AI provider and may incur charges.</p>
 <small>AI model and connection: AI settings. Sharing recollections with Narrator is a separate switch. No background or desktop clock.</small>
 {state&&<><p>Valence {state.valence} · Activation {state.activation} · Stress {state.stress}</p><small>State revision {state.revision} · processed cycles {state.cycles} · quiet cycles {state.quietCycles.valence}/{state.quietCycles.activation}/{state.quietCycles.stress}</small></>}
 <label>Quiet cycles before decay<input aria-label="Decay cycles" type="number" min={1} max={100} step={1} value={decay} disabled={disabled} onChange={e=>setDecay(e.target.value)}/></label>
 <small>Move each unreinforced axis one point toward zero after this many processing cycles. Changing this setting does not apply decay.</small>
 <label>Messages per processing cycle<input aria-label="Cycle batch size" type="number" min={1} max={8} step={1} value={batch} disabled={disabled} onChange={e=>setBatch(e.target.value)}/></label>
 <small>A shorter batch may change how a backlog evolves. Failed or empty processing does not count as a cycle.</small>
 <label>Character interpretation notes<textarea aria-label="Psychology interpretation" value={interpretation} disabled={disabled} onChange={e=>setInterpretation(e.target.value)}/></label>
 <small>Optional authored perspective or traits for analysis · {new TextEncoder().encode(interpretation).length}/4096 UTF-8 bytes. These notes are sent to AI.</small>
 {!state?.enabled&&<p>Enabling AI analysis disables authored affect and retains its stored values and appraisals. {state?"Cycle values and counters resume from the displayed state.":"Cycle state starts from zero on first configuration."}</p>}
 <div className="actions"><Button disabled={disabled||invalid} onClick={()=>onConfigure(state?.enabled??false,settings)}>Save psychology settings</Button>
 {state?.enabled?<Button disabled={disabled} onClick={()=>onConfigure(false,state.settings)}>Disable affect analysis</Button>:<Button disabled={disabled||invalid} onClick={()=>onConfigure(true,settings)}>Enable affect analysis</Button>}</div>
 <small>Disabling retains affect state and counters. Recollection extraction is independent and may still use AI; disable both for literal-only processing. Analysis sends a request when processing starts, manually or through enabled after-reply processing.</small></section>;
}
