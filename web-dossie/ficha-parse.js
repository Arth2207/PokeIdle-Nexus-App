/* Ficha oficial do jogo e fichas resumidas. Os cabeçalhos determinam as colunas de refino/IV/stat. */
(function(root){'use strict';
 const keys=['hp','atk','def','spAtk','spDef','speed'];
 const labels={'HP':0,'ATK':1,'DEF':2,'SP.ATK':3,'SPATK':3,'SP.DEF':4,'SPDEF':4,'SPD':5,'SPEED':5};
 const integer=s=>{const t=String(s??'').replace(/[^\d+-]/g,'');return t&&Number.isFinite(+t)?+t:null};
 const decimal=s=>{const m=String(s??'').match(/\d+(?:[.,]\d+)?/);return m?Number(m[0].replace(',','.')):null};
 function parse(text){
  const raw=String(text||''),lines=raw.split(/\r?\n/).map(l=>l.trim());
  const get=re=>raw.match(re)?.[1];
  const esp=raw.match(/(?:Espécie|Species)\s*:\s*([^\n(#]+?)\s*\(#(\d+)\)/i);
  const nome=esp?.[1]?.trim()||get(/^#?\s*([^\n]+?)\s*[—–]\s*(?:ficha|PokeIdle sheet)/im)||lines[0]?.replace(/^#+\s*/,'').split(/[—–]/)[0].trim();
  const f={nome,dex:esp?+esp[2]:null,dex_id:esp?+esp[2]:null,raw_text:raw,iv:Array(6).fill(null),stats:Array(6).fill(null),refino:{}};
  f.nivel=integer(get(/(?:Nível|Level)\s*:\s*([\d.,]+)/i))||integer(get(/\bNv\s*(\d+)/i));
  f.qualidade=decimal(get(/(?:Qualidade|Quality)\s*:\s*([\d.,]+)/i));
  f.potencia=get(/(?:Potência|Potency)\s*:\s*(P[1-5])/i)||get(/\b(P[1-5])\b/);
  f.shiny=/Shiny\s*:\s*(sim|yes)\b/i.test(raw);
  f.iv_total=integer(get(/(?:IV somado|Total IV)\s*:\s*([\d.,]+)/i));f.iv_max=192;
  f.hp_combate=integer(get(/(?:HP de combate|Combat HP)\s*:\s*([\d.,]+)/i));
  f.pve_power=integer(get(/Poder\s*\(⚔\)\s*:\s*([\d.,]+)/i));
  f.score=decimal(get(/Nota da Calculadora\s*:\s*([\d.,]+)/i));
  f.pokemon_id=get(/(?:Identidade|ID)\s*:\s*#?(\d+)/i);
  f.capturado_por=get(/Capturado por\s*:\s*([^\n]+)/i)?.trim();
  f.tipos=esp?(raw.match(/(?:Espécie|Species):[^\n]*/i)?.[0].match(/\b[A-Z]{3,10}\b/g)||[]):[];
  let header=null;
  for(const line of lines){
   if(!line.startsWith('|'))continue;
   const cells=line.split('|').slice(1,-1).map(x=>x.trim());
   if(cells.some(x=>/^IV\b/i.test(x))&&cells.some(x=>/^(stat|atributo)$/i.test(x))){header=cells;continue;}
   const index=labels[cells[0]?.toUpperCase().replace(/\s/g,'')];if(index==null)continue;
   let ivIndex=header?.findIndex(x=>/^IV\b/i.test(x))??-1;
   let totalIndex=header?.findIndex(x=>/stat atual|current stat|^total$|^valor$/i.test(x))??-1;
   let refIndex=header?.findIndex(x=>/^refino$/i.test(x))??-1;
   if(ivIndex<0 && cells.length===4){ivIndex=2;totalIndex=3;}
   if(ivIndex>=0)f.iv[index]=integer(cells[ivIndex]);
   if(totalIndex>=0)f.stats[index]=integer(cells[totalIndex]);
   if(refIndex>=0)f.refino[keys[index]]=integer(cells[refIndex])||0;
  }
  if(f.iv.every(x=>x!=null))f.iv_total=f.iv.reduce((a,b)=>a+b,0);
  return f;
 }
 function toGame(f,sp){
  const id=f.dex||f.dex_id||sp?.d;
  if(!id||!f.nivel||!f.qualidade||!f.potencia)throw Error('A ficha precisa de espécie, nível, qualidade e potência.');
  const ivs=f.iv||[f.iv_hp,f.iv_atk,f.iv_def,f.iv_spatk,f.iv_spdef,f.iv_spd];
  const stats=f.stats||[f.stat_hp,f.stat_atk,f.stat_def,f.stat_spatk,f.stat_spdef,f.stat_spd];
  if(!ivs?.every(x=>Number.isFinite(x)&&x>=1&&x<=32))throw Error('Cole a ficha completa, com os seis IVs individuais.');
  const p={speciesId:id,level:f.nivel,quality:f.qualidade,potencia:Number(String(f.potencia).replace(/\D/g,'')),shiny:!!f.shiny,ivs:Object.fromEntries(keys.map((k,i)=>[k,ivs[i]])),refino:f.refino||{}};
  for(const key of ['tmElemental','tmFisico','tmAoe','heldItemId'])if(Object.prototype.hasOwnProperty.call(f,key))p[key]=f[key];
  if(stats?.every(x=>Number.isFinite(x)&&x>0)){p.stats=Object.fromEntries(keys.map((k,i)=>[k,stats[i]]));p.maxHp=f.hp_combate||stats[0]*12;}
  return p;
 }
 root.NexusFicha={parse,toGame};
})(globalThis);
