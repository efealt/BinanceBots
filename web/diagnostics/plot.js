import {extent} from './analytics.js';
export const colors={buy:'var(--bid)',sell:'var(--ask)',price:'var(--accent)',neutral:'var(--pending)'};
export const number=v=>new Intl.NumberFormat('en-US',{maximumFractionDigits:8}).format(v);
export const utc=t=>new Date(t).toISOString().slice(11,23);
const ns='http://www.w3.org/2000/svg';
function node(tag,attrs={},text){const n=document.createElementNS(ns,tag); for(const [k,v] of Object.entries(attrs))n.setAttribute(k,v);if(text!=null)n.textContent=text;return n;}
export function plot(host,{series,xLabel,yLabel,time=false,domain,zero=false,bounds}) {
  host.replaceChildren();
  const all=series.flatMap(s=>s.points).filter(p=>Number.isFinite(p.x)&&Number.isFinite(p.y));
  if(!all.length){host.textContent='No observations in this interval.';return;}
  const [xmin,xmax]=domain??extent(all.map(p=>p.x));
  let [ymin,ymax]=bounds??extent(all.map(p=>p.y));
  if(zero){ymin=Math.min(0,ymin);ymax=Math.max(0,ymax);}
  const pad=(ymax-ymin)*.08||Math.max(Math.abs(ymax)*.000001,.000001);
  if(!bounds){ymin=zero&&ymin===0?0:ymin-pad;ymax+=pad;}
  const W=Math.max(360,host.clientWidth),H=285,L=80,R=28,T=26,B=52;
  const x=v=>L+(v-xmin)/(xmax-xmin||1)*(W-L-R);
  const y=v=>T+(ymax-v)/(ymax-ymin||1)*(H-T-B);
  const svg=node('svg',{viewBox:`0 0 ${W} ${H}`,role:'img','aria-label':`${yLabel} versus ${xLabel}`});
  for(let i=0;i<=4;i++){
    const yy=T+i*(H-T-B)/4,v=ymax-i*(ymax-ymin)/4;
    const decimals=Math.max(0,Math.min(8,Math.ceil(-Math.log10((ymax-ymin)/4))+1));
    const tick=number(Number(v.toFixed(decimals)));
    svg.append(node('line',{x1:L,x2:W-R,y1:yy,y2:yy,stroke:'var(--grid)'}),node('text',{x:L-9,y:yy+4,'text-anchor':'end'},tick));
    const xx=L+i*(W-L-R)/4,t=xmin+i*(xmax-xmin)/4;
    const xDecimals=Math.max(0,Math.min(8,Math.ceil(-Math.log10((xmax-xmin)/4||1))+1));
    svg.append(node('text',{x:xx,y:H-B+22,'text-anchor':'middle'},time?utc(t).slice(0,8):number(Number(t.toFixed(xDecimals)))));
  }
  svg.append(node('text',{x:L,y:H-4},xLabel));
  svg.append(node('text',{x:L,y:12},yLabel));
  const legend=document.createElement('div');legend.className='plot-legend';
  for(const s of series){
    const label=document.createElement('span');label.textContent=s.name;label.style.color=s.color;legend.append(label);
    const points=s.points.filter(p=>Number.isFinite(p.x)&&Number.isFinite(p.y));
    if(s.kind==='bar'){
      const bw=Math.max(1,(W-L-R)/Math.max(points.length,1)*.8);
      for(const p of points){
        const rect=node('rect',{x:x(p.x)-bw/2,y:Math.min(y(0),y(p.y)),width:bw,height:Math.max(.5,Math.abs(y(p.y)-y(0))),fill:s.color,opacity:.8});
        rect.append(node('title',{},`${p.label??(time?utc(p.x):number(p.x))}: ${number(p.y)}`));svg.append(rect);
      }
    } else if(s.kind==='scatter'){
      for(const p of points)svg.append(node('circle',{cx:x(p.x),cy:y(p.y),r:p.radius??2,fill:s.color,opacity:.65}));
    } else {
      const d=points.map((p,i)=>`${i?'L':'M'}${x(p.x)},${y(p.y)}`).join(' ');
      svg.append(node('path',{d,fill:'none',stroke:s.color,'stroke-width':1.7}));
    }
  }
  const readout=document.createElement('div');readout.className='plot-readout';readout.textContent='Move over the chart for exact values.';
  const cursor=node('line',{y1:T,y2:H-B,stroke:'var(--muted)','stroke-dasharray':'3 3',visibility:'hidden'});svg.append(cursor);
  svg.addEventListener('pointermove',e=>{
    const point=svg.createSVGPoint();point.x=e.clientX;point.y=e.clientY;
    const px=point.matrixTransform(svg.getScreenCTM().inverse()).x;
    const tx=xmin+(px-L)/(W-L-R)*(xmax-xmin);
    cursor.setAttribute('x1',px);cursor.setAttribute('x2',px);cursor.setAttribute('visibility','visible');
    readout.textContent=series.map(s=>{
      let best; for(const p of s.points)if(!best||Math.abs(p.x-tx)<Math.abs(best.x-tx))best=p;
      return best?`${s.name}: ${number(best.y)} · ${best.label??(time?utc(best.x):number(best.x))}`:'';
    }).filter(Boolean).join(' | ');
  });
  host.append(legend,svg,readout);
}
