import {useEffect,useState} from 'react';
import {Button} from '@rintawa/ui';
export type ExtractionSettings={enabled:boolean;batchSize:number;maxRecords:number;maxBodyBytes:number;instructions:string};
export type ExtractionState={settings:ExtractionSettings;recentIds:string[]};
const defaults:ExtractionSettings={enabled:false,batchSize:8,maxRecords:3,maxBodyBytes:1024,instructions:''};
export function ExtractionControls({state,disabled,onConfigure}:{state:ExtractionState|null;disabled:boolean;onConfigure:(settings:ExtractionSettings)=>void}){
 const settings=state?.settings??defaults;const key=JSON.stringify(settings);
 const [batch,setBatch]=useState(String(settings.batchSize)),[count,setCount]=useState(String(settings.maxRecords)),[bytes,setBytes]=useState(String(settings.maxBodyBytes)),[instructions,setInstructions]=useState(settings.instructions);
 useEffect(()=>{const s=JSON.parse(key) as ExtractionSettings;setBatch(String(s.batchSize));setCount(String(s.maxRecords));setBytes(String(s.maxBodyBytes));setInstructions(s.instructions);},[key]);
 const number=(v:string,min:number,max:number)=>v.trim()!==''&&Number.isInteger(Number(v))&&Number(v)>=min&&Number(v)<=max;
 const invalid=!number(batch,1,8)||!number(count,1,8)||!number(bytes,128,2048)||new TextEncoder().encode(instructions).length>4096;
 const draft={enabled:settings.enabled,batchSize:Number(batch),maxRecords:Number(count),maxBodyBytes:Number(bytes),instructions};
 return <section aria-label="Recollection extraction"><h2>Extract recollections</h2>
 <p>{settings.enabled?'Enabled':'Disabled'} · Extract a few subjective recollections from new source batches. Transcript text does not prove facts or witnessing. Input goes to your configured AI provider and may incur charges.</p>
 <small>Future unprocessed source only. Prior literal observations are not reprocessed automatically. With affect analysis enabled, both use one request and the smaller batch size.</small>
 <label>Messages per extraction batch<input aria-label="Extraction batch size" type="number" min={1} max={8} step={1} value={batch} disabled={disabled} onChange={e=>setBatch(e.target.value)}/></label>
 <label>Maximum recollections per batch<input aria-label="Extraction record count" type="number" min={1} max={8} step={1} value={count} disabled={disabled} onChange={e=>setCount(e.target.value)}/></label>
 <label>Maximum generated body bytes<input aria-label="Extraction body limit" type="number" min={128} max={2048} step={1} value={bytes} disabled={disabled} onChange={e=>setBytes(e.target.value)}/></label>
 <label>Extraction notes<textarea aria-label="Extraction instructions" value={instructions} disabled={disabled} onChange={e=>setInstructions(e.target.value)}/></label>
 <small>{new TextEncoder().encode(instructions).length}/4096 UTF-8 bytes · Combined generated bodies are limited to 8192 bytes. Model/connection and output-token limits use AI settings.</small>
 <div className="actions"><Button disabled={disabled||invalid} onClick={()=>onConfigure(draft)}>Save extraction settings</Button>{settings.enabled?<Button disabled={disabled} onClick={()=>onConfigure({...settings,enabled:false})}>Disable recollection extraction</Button>:<Button disabled={disabled||invalid} onClick={()=>onConfigure({...draft,enabled:true})}>Enable recollection extraction</Button>}</div>
 <small>Disabling keeps existing records and corrections. Sharing with Narrator and processing after replies are separate switches. Recent extracted records are preferred within narration's existing budget; all retained records remain browsable. Extraction does not feed its own output back as fresh source.</small></section>;
}
