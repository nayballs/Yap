//! **Yap's own scripts for acting on a page** (run in the page's own
//! webview through `view::run_script`; never a script a page or a model
//! wrote — the model's words only ever go in as a JSON string).
//!
//! One small library ([`LIB`]) and an operation:
//! - `snap`: the page's title, address, main text, headings, and its links,
//!   buttons and fields (visible ones, in the page's order, at most
//!   `rules::MAX_ELEMENTS`), each with its ref; its forms (a password field?
//!   where it goes, its submit button's words); where it's scrolled to.
//! - `describe`: one element by its ref, as it is now, with its form; `at`:
//!   the element at a point of the view; `active`: the one with the focus.
//! - `pos`: where the page is scrolled to, its view's size and the headings
//!   in view; `blocks`: its visible text blocks, each with a ref.
//! - `click`, `type`, `scroll`, `clickAt`, `hover`, `check`, `key`: the
//!   action — only if the element is still
//!   the one described (its role and name, checked in the same script run as
//!   the action: a page can't swap it in between).
//!
//! **Refs** live in the page (`window.__yapRefs`: a `WeakMap` from element to
//! number): an element keeps its number for as long as the page does, new
//! ones get the next; a navigation starts afresh (the page's own JS world
//! goes). A page could read or tamper with the map: the worst it could do is
//! point a ref elsewhere, which the role-and-name check before every action
//! refuses ("changed"), and Rust has checked that element's rules first.
//!
//! **Never a secret back**: a password field's value, a card's, a code's
//! (by type, `autocomplete` or name) is left out (`masked`), so it never
//! reaches Rust, the log or the model.

/// The library every operation runs with (`op`, `a` in scope).
const LIB: &str = r#"
var W=window,D=document;
var R=W.__yapRefs;
if(!R||!R.m||!R.els){R={m:new WeakMap(),els:[null]};try{Object.defineProperty(W,'__yapRefs',{value:R,configurable:true,enumerable:false,writable:true});}catch(e){W.__yapRefs=R;}}
function sq(s){return (s||'').replace(/\s+/g,' ').trim();}
function refOf(el){var r=R.m.get(el);if(!r){r=R.els.length;R.els.push(el);R.m.set(el,r);}return r;}
function vis(el){if(!el||!el.isConnected)return false;if(!el.getClientRects().length)return false;var s=getComputedStyle(el);if(s.visibility==='hidden'||s.display==='none')return false;return !el.closest('[aria-hidden="true"],[hidden],[inert]');}
var F=Array.prototype.slice.call(D.forms);
function tag(el){return el.tagName.toLowerCase();}
function itype(el){return tag(el)==='input'?(el.getAttribute('type')||'text').toLowerCase():'';}
function roleOf(el){var r=(el.getAttribute('role')||'').toLowerCase();if(r)return r;var t=tag(el),ty=itype(el);
if(t==='a')return 'link';if(t==='button'||t==='summary')return 'button';if(t==='select')return 'combobox';if(t==='textarea')return 'textbox';
if(t==='input'){if(['submit','button','reset','image'].indexOf(ty)>=0)return 'button';if(ty==='checkbox')return 'checkbox';if(ty==='radio')return 'radio';if(ty==='search')return 'searchbox';if(ty==='range')return 'slider';if(ty==='number')return 'spinbutton';return 'textbox';}
if(el.isContentEditable)return 'textbox';return t;}
function labelOf(el){var out=[];if(el.labels){for(var i=0;i<el.labels.length;i++)out.push(el.labels[i].innerText||el.labels[i].textContent);}if(!out.length){var l=el.closest('label');if(l)out.push(l.innerText||l.textContent);}return sq(out.join(' ')).slice(0,120);}
function nameOf(el){var x=sq(el.getAttribute('aria-label'));if(x)return x;var lb=el.getAttribute('aria-labelledby');if(lb){var t=sq(lb.split(/\s+/).map(function(i){var e=D.getElementById(i);return e?(e.innerText||e.textContent):'';}).join(' '));if(t)return t;}
var t2=tag(el),ty=itype(el);if(t2==='input'&&['submit','button','reset'].indexOf(ty)>=0)return sq(el.value)||ty;if(t2==='input'&&ty==='image')return sq(el.alt);
var l=labelOf(el);if(l)return l;if(t2==='input'||t2==='textarea'||t2==='select')return sq(el.getAttribute('placeholder')||el.title||'');
var tx=sq(el.innerText||el.textContent);if(tx)return tx.slice(0,120);var img=el.querySelector('img[alt]');if(img)return sq(img.alt);return sq(el.title||'');}
function words(el){return [el.getAttribute('name'),el.id,el.getAttribute('placeholder'),el.getAttribute('aria-label'),labelOf(el)].join(' ').replace(/[\u00AD\u180E\u200B-\u200F\u202A-\u202E\u2060-\u2064\u2066-\u2069\uFEFF]/g,'').replace(/([a-z])([A-Z])/g,'$1 $2');}
function secret(el){var ty=itype(el),ac=(el.getAttribute('autocomplete')||'').toLowerCase();if(ty==='password'||/(^|\s)(cc-|current-password|new-password|one-time-code)/.test(ac))return true;
return /card|cvv|cvc|csc|iban|sort.?code|account.?n(o|um)|security.?code|pass(word|code|wort)|kennwort|mot.?de.?passe|contrase(ñ|n)a|senha|wachtwoord|\bpin\b|otp|\btan\b|karte|carte|tarjeta|cart(ã|a)o|carta|kaart|cryptogram|pr(ü|u)f|konto|compte|cuenta|conto|rekening|bankleitzahl|sicherheit|s(é|e)curit|seguridad|seguran|sicurezza|beveiliging/i.test(words(el));}
var CARD=/\b(card|cards|cardnumber|ccnum|ccnumber|creditcard|debitcard|cvv2?|cvc2?|csc|cardholder|karte|kreditkarte|karteninhaber|pr(ü|u)fnummer|carte|cryptogramme|tarjeta|cart(ã|a)o|carta|kaarthouder|pasnummer)\b|kartennummer|kaartnummer/i;
var PAYF=/(^|\.)(stripe\.com|adyen\.com|braintreegateway\.com|paypal\.com|checkout\.com|mollie\.com|squareup\.com|klarna\.com|worldpay\.com)$/i;
function payFrame(f){try{return PAYF.test(new URL(f.getAttribute('src'),location.href).hostname);}catch(e){return false;}}
function cardField(el){var ac=(el.getAttribute('autocomplete')||'').toLowerCase();return /(^|\s)cc-/.test(ac)||CARD.test(words(el));}
function takesCard(root){var ins=root.querySelectorAll('input,select');for(var i=0;i<ins.length;i++){if(itype(ins[i])!=='hidden'&&cardField(ins[i]))return true;}var fr=root.querySelectorAll('iframe[src]');for(var j=0;j<fr.length;j++){if(payFrame(fr[j]))return true;}return false;}
var PRICE=/[£€$¥₹₩₽₺₪¢₱₫฿]\s?\d|\d\s?[£€$¥₹₩₽₺₪¢₱₫฿]|\b(usd|eur|gbp|chf|jpy|cad|aud|sek|nok|dkk|pln|brl|mxn|kr|zł)\s?\d|\d\s?(usd|eur|gbp|chf|kr|zł)\b/i;
function inSearch(el){var h=(el.getAttribute('enterkeyhint')||'').toLowerCase(),m=(el.getAttribute('inputmode')||'').toLowerCase();return h==='search'||m==='search'||!!el.closest('[role=search],search');}
function submitBtn(f){return f?f.querySelector('button:not([type=button]):not([type=reset]),input[type=submit],input[type=image]'):null;}
function formsInfo(){return F.map(function(f,i){var b=submitBtn(f);return {form:i,password:!!f.querySelector('input[type=password]'),action:f.action||'',method:(f.getAttribute('method')||'get').toLowerCase(),submit:b?nameOf(b):'',payment:takesCard(f),price:PRICE.test(sq(f.innerText).slice(0,4000))};});}
function desc(el){var t=tag(el),ty=itype(el),m=secret(el),ed=el.isContentEditable&&t!=='input'&&t!=='textarea';
var d={ref:refOf(el),role:roleOf(el),name:nameOf(el).slice(0,120),type:ty,autocomplete:el.getAttribute('autocomplete')||'',field:el.getAttribute('name')||'',id:el.id||'',label:labelOf(el),placeholder:el.getAttribute('placeholder')||'',href:t==='a'?(el.href||''):'',disabled:!!el.disabled||el.getAttribute('aria-disabled')==='true',form:el.form?F.indexOf(el.form):-1,submit:(t==='button'&&(el.getAttribute('type')||'submit').toLowerCase()==='submit'&&!!el.form)||(t==='input'&&(ty==='submit'||ty==='image')),masked:m,value:'',options:[],search:inSearch(el),editable:ed};
if(ty==='checkbox'||ty==='radio'||d.role==='checkbox'||d.role==='radio'||d.role==='switch'){d.checked=ty?!!el.checked:el.getAttribute('aria-checked')==='true';}
else if(t==='select'){var o=el.options[el.selectedIndex];d.value=o?sq(o.text):'';for(var i=0;i<el.options.length&&i<20;i++)d.options.push(sq(el.options[i].text));}
else if(!m&&(t==='input'||t==='textarea')&&['submit','button','reset','image'].indexOf(ty)<0){d.value=String(el.value||'').slice(0,200);}
else if(!m&&el.isContentEditable){d.value=sq(el.innerText).slice(0,200);}
return d;}
var SEL='a[href],button,input:not([type=hidden]),select,textarea,summary,[role=button],[role=link],[role=checkbox],[role=radio],[role=tab],[role=menuitem],[role=switch],[role=combobox],[role=textbox],[role=searchbox],[role=option],[contenteditable=""],[contenteditable=true]';
function scroller(){var se=D.scrollingElement||D.documentElement;if(!se||se.scrollHeight>W.innerHeight+2)return null;var best=null,area=0,all=D.querySelectorAll('body *');for(var i=0;i<all.length&&i<6000;i++){var e=all[i];if(e.scrollHeight>e.clientHeight+2){var o=getComputedStyle(e).overflowY;if(o==='auto'||o==='scroll'){var ar=e.clientWidth*e.clientHeight;if(ar>area){area=ar;best=e;}}}}return best;}
function scrollAt(){var de=D.documentElement,sc=scroller();if(sc)return {x:Math.round(W.scrollX),y:Math.round(sc.scrollTop),w:Math.round(W.innerWidth),h:Math.round(sc.clientHeight),max:Math.max(0,Math.round(sc.scrollHeight-sc.clientHeight)),dpr:W.devicePixelRatio||1};
return {x:Math.round(W.scrollX),y:Math.round(W.scrollY),w:Math.round(W.innerWidth),h:Math.round(W.innerHeight),max:Math.max(0,Math.round((de?de.scrollHeight:0)-W.innerHeight)),dpr:W.devicePixelRatio||1};}
function inView(){var out=[],hs=D.querySelectorAll('h1,h2,h3,h4,[role=heading]');for(var i=0;i<hs.length&&out.length<8;i++){var r=hs[i].getBoundingClientRect();if(r.bottom>0&&r.top<W.innerHeight&&vis(hs[i])){var t=sq(hs[i].innerText);if(t)out.push(t.slice(0,100));}}return out;}
function heads(){return [].slice.call(D.querySelectorAll('h1,h2,h3')).slice(0,30).map(function(h){return sq(h.innerText);}).filter(Boolean);}
function find(){var el=R.els[a.ref];return el&&el.isConnected?el:null;}
function same(el){var d=desc(el);return d.role===a.role&&d.name===a.name;}
function fire(el,k){el.dispatchEvent(new Event(k,{bubbles:true}));}
function base(el){var b={password:!!D.querySelector('input[type=password]'),payment:takesCard(D),title:sq(D.title),headings:heads(),url:location.href};if(!el){b.el=null;return b;}b.el=desc(el);b.form=el.form?formsInfo()[F.indexOf(el.form)]:null;return b;}
if(op==='pos')return {r:'ok',scroll:scrollAt(),inView:inView()};
if(op==='blocks'){var bl=D.querySelectorAll('h1,h2,h3,h4,h5,h6,[role=heading],p,li,td,th,dt,dd,caption,figcaption,blockquote,pre,summary,label,legend,a,button,option'),bs=[],tot=0;
for(var i=0;i<bl.length&&bs.length<a.max;i++){var e=bl[i];if(!vis(e))continue;var t=sq(e.innerText||e.textContent);if(!t)continue;t=t.slice(0,a.chars);tot+=t.length;if(tot>a.total)break;bs.push({ref:refOf(e),tag:tag(e),role:(e.getAttribute('role')||'').toLowerCase(),text:t});}
return {r:'ok',blocks:bs};}
if(op==='at'){var hit=D.elementFromPoint(Number(a.x),Number(a.y));return base(hit?(hit.closest(SEL)||hit):null);}
if(op==='active'){var ae=D.activeElement;return base(ae&&ae!==D.body&&ae!==D.documentElement?ae:null);}
if(op==='clickAt'){var el=find();if(!el)return {r:'gone'};if(!same(el))return {r:'changed'};if(el.disabled)return {r:'disabled'};var hit=D.elementFromPoint(Number(a.x),Number(a.y));if(!hit||!(hit===el||el.contains(hit)))return {r:'changed'};
var o={bubbles:true,cancelable:true,composed:true,clientX:Number(a.x),clientY:Number(a.y),view:W,button:0};try{hit.dispatchEvent(new PointerEvent('pointerdown',o));}catch(e){}hit.dispatchEvent(new MouseEvent('mousedown',o));try{el.focus();}catch(e){}try{hit.dispatchEvent(new PointerEvent('pointerup',o));}catch(e){}hit.dispatchEvent(new MouseEvent('mouseup',o));
var tg=tag(el)==='a'?(el.getAttribute('target')||'').toLowerCase():'';if(tg&&tg!=='_self'&&tg!=='_parent'&&tg!=='_top'&&/^https?:/i.test(el.href)){location.assign(el.href);return {r:'ok',went:true};}
hit.dispatchEvent(new MouseEvent('click',o));return {r:'ok'};}
if(op==='hover'){var el=find();if(!el)return {r:'gone'};if(!same(el))return {r:'changed'};try{el.scrollIntoView({block:'center'});}catch(e){}var br=el.getBoundingClientRect(),o={bubbles:true,cancelable:true,clientX:br.left+br.width/2,clientY:br.top+br.height/2,view:W};
['pointerover','pointerenter','mouseover','mouseenter','pointermove','mousemove'].forEach(function(k){try{el.dispatchEvent(k.indexOf('pointer')===0?new PointerEvent(k,o):new MouseEvent(k,o));}catch(e){}});return {r:'ok'};}
if(op==='check'){var el=find();if(!el)return {r:'gone'};if(!same(el))return {r:'changed'};if(el.disabled)return {r:'disabled'};var isOn=function(){return itype(el)?!!el.checked:el.getAttribute('aria-checked')==='true';};if(isOn()!==!!a.on){try{el.scrollIntoView({block:'center'});}catch(e){}el.click();}return {r:'ok',checked:isOn()};}
if(op==='key'){var t0=a.ref?find():null;if(a.ref){if(!t0)return {r:'gone'};if(!same(t0))return {r:'changed'};try{t0.focus();}catch(e){}}var kd=[];
for(var ki=0;ki<a.keys.length;ki++){var k=a.keys[ki],cur=D.activeElement||D.body;
if(k==='Tab'||k==='Shift+Tab'){var fs=[].slice.call(D.querySelectorAll('a[href],button,input:not([type=hidden]),select,textarea,[tabindex],[contenteditable=""],[contenteditable=true]')).filter(function(e){return vis(e)&&!e.disabled&&e.tabIndex>=0;});var j=fs.indexOf(cur);var nx=fs[k==='Tab'?j+1:(j<0?fs.length-1:j-1)];if(nx){try{nx.focus();}catch(e){}}kd.push(k);continue;}
var ko={key:k==='Space'?' ':k,code:k==='Space'?'Space':k,keyCode:({Enter:13,Escape:27,Space:32,ArrowUp:38,ArrowDown:40,ArrowLeft:37,ArrowRight:39,PageUp:33,PageDown:34,Home:36,End:35})[k]||0,bubbles:true,cancelable:true};
var go=cur.dispatchEvent(new KeyboardEvent('keydown',ko));
if(go){var ct=tag(cur),cr=roleOf(cur);
if(k==='Enter'){if(cur.form&&ct==='input'){if(cur.form.requestSubmit)cur.form.requestSubmit();else cur.form.submit();}else if(ct==='a'||ct==='button'||cr==='button'||cr==='link'){cur.click();}else{cur.dispatchEvent(new KeyboardEvent('keypress',ko));}}
else if(k==='Space'&&(ct==='button'||itype(cur)==='checkbox'||itype(cur)==='radio'||cr==='button')){cur.click();}
else if(ct==='select'&&(k==='ArrowDown'||k==='ArrowUp')){var si=cur.selectedIndex+(k==='ArrowDown'?1:-1);if(si>=0&&si<cur.options.length){cur.selectedIndex=si;fire(cur,'input');fire(cur,'change');}}
else if(cur===D.body||cur===D.documentElement||ct==='a'){var sc=scroller(),sh=Math.round(W.innerHeight*0.85),by=({PageDown:sh,Space:sh,PageUp:-sh,ArrowDown:40,ArrowUp:-40})[k];if(by){if(sc)sc.scrollBy(0,by);else W.scrollBy(0,by);}else if(k==='Home'){if(sc)sc.scrollTop=0;else W.scrollTo(W.scrollX,0);}else if(k==='End'){if(sc)sc.scrollTop=sc.scrollHeight;else W.scrollTo(W.scrollX,D.documentElement.scrollHeight);}}}
cur.dispatchEvent(new KeyboardEvent('keyup',ko));kd.push(k);}
var fe=D.activeElement;return {r:'ok',keys:kd,focused:fe&&fe!==D.body&&fe!==D.documentElement?desc(fe):null,scroll:scrollAt()};}
if(op==='snap'){var main=D.querySelector('main,article,[role=main]')||D.body;var all=D.querySelectorAll(SEL),els=[],more=false;
for(var i=0;i<all.length;i++){if(!vis(all[i]))continue;if(els.length>=a.max){more=true;break;}els.push(desc(all[i]));}
return {title:sq(D.title),url:location.href,text:main?sq(main.innerText).slice(0,a.text):'',headings:heads(),elements:els,forms:formsInfo(),password:!!D.querySelector('input[type=password]'),payment:takesCard(D),more:more,scroll:scrollAt()};}
if(op==='describe'){var el=find();var base={password:!!D.querySelector('input[type=password]'),payment:takesCard(D),title:sq(D.title),headings:heads(),url:location.href};
if(!el){base.el=null;return base;}base.el=desc(el);base.form=el.form?formsInfo()[F.indexOf(el.form)]:null;return base;}
if(op==='scroll'){if(a.ref){var s=find();if(!s)return {r:'gone'};s.scrollIntoView({block:'center'});}
else{var sc=scroller(),d=a.to||'down',n=Math.max(0.1,Math.min(10,Number(a.amount)||1)),step=Math.round((sc?sc.clientHeight:W.innerHeight)*0.85*n);
if(d==='top'){if(sc)sc.scrollTop=0;else W.scrollTo(W.scrollX,0);}else if(d==='bottom'){if(sc)sc.scrollTop=sc.scrollHeight;else W.scrollTo(W.scrollX,D.documentElement.scrollHeight);}else{if(sc)sc.scrollBy(0,(d==='up'?-1:1)*step);else W.scrollBy(0,(d==='up'?-1:1)*step);}}
return {r:'ok',scroll:scrollAt(),inView:inView()};}
var el=find();if(!el)return {r:'gone'};if(!same(el))return {r:'changed'};if(el.disabled)return {r:'disabled'};
var caretIn=(function(){var s=W.getSelection();return !!(s&&s.rangeCount&&el.contains(s.anchorNode));})();
try{el.scrollIntoView({block:'center'});}catch(e){}try{el.focus();}catch(e){}
if(op==='click'){var tg=tag(el)==='a'?(el.getAttribute('target')||'').toLowerCase():'';
if(tg&&tg!=='_self'&&tg!=='_parent'&&tg!=='_top'&&/^https?:/i.test(el.href)){location.assign(el.href);return {r:'ok',went:true};}
el.click();return {r:'ok'};}
if(op==='type'){var t=tag(el);
if(t==='select'){var want=String(a.text).toLowerCase().trim(),pick=null;for(var i=0;i<el.options.length;i++){var o=el.options[i];if(sq(o.text).toLowerCase()===want||String(o.value).toLowerCase()===want){pick=o;break;}}
if(!pick){for(var j=0;j<el.options.length;j++){if(sq(el.options[j].text).toLowerCase().indexOf(want)>=0){pick=el.options[j];break;}}}if(!pick)return {r:'no-option'};el.value=pick.value;fire(el,'input');fire(el,'change');}
else if(t==='input'||t==='textarea'){var proto=t==='textarea'?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;var set=Object.getOwnPropertyDescriptor(proto,'value').set;set.call(el,String(a.text));fire(el,'input');fire(el,'change');}
else if(el.isContentEditable){var sel=W.getSelection(),txt=String(a.text);
if(txt){if(!caretIn||!sel.rangeCount){var rg=D.createRange();rg.selectNodeContents(el);rg.collapse(false);sel.removeAllRanges();sel.addRange(rg);var had=el.innerText||'';if(had.trim()&&!/\s$/.test(had))txt=' '+txt;}
var done=false;try{done=D.execCommand('insertText',false,txt);}catch(e){}
if(!done){var r2=sel.getRangeAt(0);r2.insertNode(D.createTextNode(txt));r2.collapse(false);fire(el,'input');}}}
else return {r:'not-a-field'};
var ins=el.isContentEditable&&t!=='input'&&t!=='textarea';
var now=secret(el)?'':(t==='select'?sq(el.options[el.selectedIndex]?el.options[el.selectedIndex].text:''):(ins?sq(el.innerText):String(el.value||'')).slice(0,200));
if(a.submit){if(el.form){if(el.form.requestSubmit)el.form.requestSubmit();else el.form.submit();}else{['keydown','keypress','keyup'].forEach(function(k){el.dispatchEvent(new KeyboardEvent(k,{key:'Enter',code:'Enter',keyCode:13,which:13,bubbles:true}));});}}
return {r:'ok',value:now,inserted:ins};}
return {r:'unknown'};
"#;

/// The script for `op` with `args` (JSON: safe for any words in it). Pure.
fn build(op: &str, args: serde_json::Value) -> String {
    let op = serde_json::to_string(op).unwrap_or_else(|_| "\"\"".into());
    let mut s = String::with_capacity(LIB.len() + 200);
    s.push_str("(function(op,a){");
    s.push_str(LIB);
    s.push_str("})(");
    s.push_str(&op);
    s.push(',');
    s.push_str(&args.to_string());
    s.push(')');
    s
}

/// Read the page with its elements: its text cut at `text` characters, at
/// most `max` elements.
pub fn snap(text: usize, max: usize) -> String {
    build("snap", serde_json::json!({ "text": text, "max": max }))
}

/// Describe element `r` as it is now.
pub fn describe(r: u32) -> String {
    build("describe", serde_json::json!({ "ref": r }))
}

/// Click element `r`, if it's still `role` "`name`".
pub fn click(r: u32, role: &str, name: &str) -> String {
    build("click", serde_json::json!({ "ref": r, "role": role, "name": name }))
}

/// Type `text` into element `r` (a select: the option with those words), if
/// it's still `role` "`name`"; `submit`: its form submitted after (Enter).
pub fn type_text(r: u32, role: &str, name: &str, text: &str, submit: bool) -> String {
    build("type", serde_json::json!({ "ref": r, "role": role, "name": name, "text": text, "submit": submit }))
}

/// Scroll the page `to` "up", "down" (`amount` screens), "top" or "bottom",
/// or element `r` into view (centred). Its answer: where it is now and the
/// headings in view.
pub fn scroll(to: &str, amount: f64, r: Option<u32>) -> String {
    build("scroll", serde_json::json!({ "to": to, "amount": amount, "ref": r }))
}

/// Where the page is scrolled to, its view's size and the headings in view.
pub fn pos() -> String {
    build("pos", serde_json::json!({}))
}

/// The page's visible text blocks (headings, paragraphs, list items, cells,
/// links, buttons…), each with a ref: at most `max`, `chars` characters
/// each, `total` in all.
pub fn blocks(max: usize, chars: usize, total: usize) -> String {
    build("blocks", serde_json::json!({ "max": max, "chars": chars, "total": total }))
}

/// The element at point (`x`, `y`) of the view (CSS pixels), described as
/// `describe` does (its nearest link, button or field, else itself).
pub fn at(x: f64, y: f64) -> String {
    build("at", serde_json::json!({ "x": x, "y": y }))
}

/// The element with the focus, described as `describe` does.
pub fn active() -> String {
    build("active", serde_json::json!({}))
}

/// A click at (`x`, `y`) on element `r`, if it's still `role` "`name`" and
/// still the one at that point (pointer and mouse events there, then the
/// click).
pub fn click_at(r: u32, role: &str, name: &str, x: f64, y: f64) -> String {
    build("clickAt", serde_json::json!({ "ref": r, "role": role, "name": name, "x": x, "y": y }))
}

/// The pointer over element `r`, if it's still `role` "`name`".
pub fn hover(r: u32, role: &str, name: &str) -> String {
    build("hover", serde_json::json!({ "ref": r, "role": role, "name": name }))
}

/// A checkbox, radio or switch `r` ticked (`on`) or not, if it's still
/// `role` "`name`" (a click when it isn't already so).
pub fn check(r: u32, role: &str, name: &str, on: bool) -> String {
    build("check", serde_json::json!({ "ref": r, "role": role, "name": name, "on": on }))
}

/// `keys` pressed in turn (Enter, Tab, Shift+Tab, Escape, Space, the arrows,
/// PageUp, PageDown, Home, End) in element `r` (if it's still `role`
/// "`name`"), else in the one with the focus.
pub fn keys(keys: &[String], r: Option<(u32, &str, &str)>) -> String {
    match r {
        Some((r, role, name)) => build("key", serde_json::json!({ "keys": keys, "ref": r, "role": role, "name": name })),
        None => build("key", serde_json::json!({ "keys": keys, "ref": 0 })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_models_words_go_in_as_a_json_string_never_as_code() {
        let s = type_text(4, "textbox", "From", "Kellbrook\"});alert(1);//</script>", true);
        assert!(s.starts_with("(function(op,a){"), "{s}");
        // The arguments: one JSON object after the operation's name, the words a string in it.
        let args = s.rsplit_once("})(\"type\",").map(|(_, a)| a.trim_end_matches(')')).unwrap();
        let v: serde_json::Value = serde_json::from_str(args).unwrap();
        assert_eq!(v, serde_json::json!({ "ref": 4, "role": "textbox", "name": "From", "text": "Kellbrook\"});alert(1);//</script>", "submit": true }));
        assert!(snap(2500, 150).contains("})(\"snap\",{"));
        assert!(describe(7).contains("})(\"describe\",{\"ref\":7})"));
        assert!(scroll("up", 1.0, None).contains("\"to\":\"up\""));
        assert!(click_at(3, "button", "Go", 120.0, 48.5).contains("})(\"clickAt\",{"));
        assert!(keys(&["Enter".into()], None).contains("\"keys\":[\"Enter\"]"));
        // A point click runs before the shared path (which scrolls the element into view: the point would move).
        assert!(LIB.find("op==='clickAt'").unwrap() < LIB.find("var caretIn").unwrap());
        // Never a secret's value: the library masks them by type, autocomplete and name.
        assert!(LIB.contains("current-password|new-password|one-time-code") && LIB.contains("masked:m"));
        // Every action checks the element is still the one described first.
        assert!(LIB.contains("if(!same(el))return {r:'changed'}"));
    }
}
