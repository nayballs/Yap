//! **Yap's own scripts for acting on a page** (run in the page's own
//! webview through `view::run_script`; never a script a page or a model
//! wrote — the model's words only ever go in as a JSON string).
//!
//! One small library ([`LIB`]) and an operation:
//! - `snap`: the page's title, address, main text, headings, and its links,
//!   buttons and fields (visible ones, in the page's order, at most
//!   `rules::MAX_ELEMENTS`), each with its ref; its forms (a password field?
//!   where it goes, its submit button's words); where it's scrolled to.
//! - `describe`: one element by its ref, as it is now, with its form and the
//!   highest ref the page has given (a ref past it never existed); `at`: the
//!   element at a point of the view; `active`: the one with the focus.
//! - `pos`: where the page is scrolled to, its view's size and the headings
//!   in view; `where`: whether an element is in view; `blocks`: its visible
//!   text blocks, each with a ref.
//! - `click`, `type`, `choose`, `scroll`, `clickAt`, `hover`, `check`, `key`:
//!   the action — only if the element is still the one described (its role
//!   and name, checked in the same script run as the action: a page can't
//!   swap it in between). `choose`: a select's option, a radio button of a
//!   group, a combobox's or listbox's `[role=option]`, by its words (8 Nov
//!   2026: a dropdown choice was typed, and asked about as words from
//!   nowhere).
//! - `watch`, then `changes` (8 Nov 2026): a `MutationObserver` from just
//!   before an action until the page has settled, and what it saw — the
//!   scroll and the address before and now, the field the focus moved into,
//!   what opened or closed over the page (a dialog, a menu, a list, a popup),
//!   how many elements changed in view (`rules::Changes`).
//!
//! **Scrolling is instant** (8 Nov 2026: on a page with `scroll-behavior:
//! smooth` a scroll answered from where it started, mid-way): every scroll
//! Yap makes is `behavior: "instant"`, which overrides the page's CSS, and a
//! focus never scrolls on its own (`preventScroll`); `backend::still` waits
//! out a scroll the page makes itself. **A hover is let go** (a pointer
//! moved away) when Escape is pressed or another element is hovered, so a
//! hover's popup closes as it would for a person.
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
//!
//! **Inside open shadow roots too** (8 Nov 2026, Yap Lab on MDN's web
//! component search; `pages.rs` `deep_js!`): the elements, blocks, forms,
//! headings and overlays are found through them in the page's order; the
//! element at a point and the one with the focus are the real ones inside
//! (not the host); a label is found in the element's own root
//! (`aria-labelledby`, `<label for>`, the `<label>` around it, else its
//! host's `aria-label` / `label` for a field); events that cross a shadow
//! boundary for a person (`input`, keys, the pointer's bubbling ones) do;
//! the watch observes every open root, and roots that appear after the
//! action. A custom element with nothing Yap can see in it (a closed root,
//! most likely) is `closed`. A page with no shadow root reads exactly as
//! before.

/// The library every operation runs with (`op`, `a` in scope).
const LIB: &str = concat!(
    deep_js!(),
    r#"
var W=window,D=document;
var R=W.__yapRefs;
if(!R||!R.m||!R.els){R={m:new WeakMap(),els:[null]};try{Object.defineProperty(W,'__yapRefs',{value:R,configurable:true,enumerable:false,writable:true});}catch(e){W.__yapRefs=R;}}
function sq(s){return (s||'').replace(/\s+/g,' ').trim();}
function refOf(el){var r=R.m.get(el);if(!r){r=R.els.length;R.els.push(el);R.m.set(el,r);}return r;}
function vis(el){if(!el||!el.isConnected)return false;if(!el.getClientRects().length)return false;var s=getComputedStyle(el);if(s.visibility==='hidden'||s.display==='none')return false;return !up(el,'[aria-hidden="true"],[hidden],[inert]');}
function shown(el){if(!vis(el))return false;var b=el.getBoundingClientRect();return b.width>0&&b.height>0&&b.bottom>0&&b.right>0&&b.top<W.innerHeight&&b.left<W.innerWidth;}
function sTo(sc,y){try{if(sc)sc.scrollTo({top:y,behavior:'instant'});else W.scrollTo({left:W.scrollX,top:y,behavior:'instant'});}catch(e){if(sc)sc.scrollTop=y;else W.scrollTo(W.scrollX,y);}}
function sBy(sc,dy){try{if(sc)sc.scrollBy({top:dy,behavior:'instant'});else W.scrollBy({top:dy,behavior:'instant'});}catch(e){if(sc)sc.scrollTop+=dy;else W.scrollBy(0,dy);}}
function into(el){try{el.scrollIntoView({block:'center',inline:'nearest',behavior:'instant'});}catch(e){try{el.scrollIntoView();}catch(e2){}}}
var F_=null;function Fs(){if(!F_)F_=[].slice.call(deep()?qa('form'):D.forms);return F_;}
function tag(el){return el.tagName.toLowerCase();}
function isField(el){var t=tag(el);return t==='input'||t==='textarea'||t==='select'||el.isContentEditable;}
function itype(el){return tag(el)==='input'?(el.getAttribute('type')||'text').toLowerCase():'';}
function roleOf(el){var r=(el.getAttribute('role')||'').toLowerCase();if(r)return r;var t=tag(el),ty=itype(el);
if(t==='a')return 'link';if(t==='button'||t==='summary')return 'button';if(t==='select')return 'combobox';if(t==='textarea')return 'textbox';
if(t==='input'){if(['submit','button','reset','image'].indexOf(ty)>=0)return 'button';if(ty==='checkbox')return 'checkbox';if(ty==='radio')return 'radio';if(ty==='search')return 'searchbox';if(ty==='range')return 'slider';if(ty==='number')return 'spinbutton';return 'textbox';}
if(el.isContentEditable)return 'textbox';return t;}
function labelOf(el){var out=[],f=isField(el);if(el.labels){for(var i=0;i<el.labels.length;i++)out.push(textOf(el.labels[i]));}if(!out.length){var l=el.closest('label')||(f?up(el,'label'):null);if(l)out.push(textOf(l));}
if(!out.length&&f){var h=hostOf(el);if(h){var hl=sq(h.getAttribute('aria-label')||h.getAttribute('label'));if(hl)out.push(hl);}}return sq(out.join(' ')).slice(0,120);}
function lbl(el){var v=el.getAttribute('aria-labelledby');if(!v)return '';return sq(v.split(/\s+/).map(function(i){var e=i?byId(el,i):null;return e?textOf(e):'';}).join(' '));}
function nameOf(el){var x=sq(el.getAttribute('aria-label'));if(x)return x;var t=lbl(el);if(t)return t;
var t2=tag(el),ty=itype(el);if(t2==='input'&&['submit','button','reset'].indexOf(ty)>=0)return sq(el.value)||ty;if(t2==='input'&&ty==='image')return sq(el.alt);
var l=labelOf(el);if(l)return l;if(t2==='input'||t2==='textarea'||t2==='select')return sq(el.getAttribute('placeholder')||el.title||'');
var tx=textOf(el);if(tx)return tx.slice(0,120);var img=q1('img[alt]',el);if(img)return sq(img.alt);return sq(el.title||'');}
function words(el){return [el.getAttribute('name'),el.id,el.getAttribute('placeholder'),el.getAttribute('aria-label'),lbl(el),labelOf(el)].join(' ').replace(/[\u00AD\u180E\u200B-\u200F\u202A-\u202E\u2060-\u2064\u2066-\u2069\uFEFF]/g,'').replace(/([a-z])([A-Z])/g,'$1 $2');}
function secret(el){var ty=itype(el),ac=(el.getAttribute('autocomplete')||'').toLowerCase();if(ty==='password'||/(^|\s)(cc-|current-password|new-password|one-time-code)/.test(ac))return true;
return /card|cvv|cvc|csc|iban|sort.?code|account.?n(o|um)|security.?code|pass(word|code|wort)|kennwort|mot.?de.?passe|contrase(ñ|n)a|senha|wachtwoord|\bpin\b|otp|\btan\b|karte|carte|tarjeta|cart(ã|a)o|carta|kaart|cryptogram|pr(ü|u)f|konto|compte|cuenta|conto|rekening|bankleitzahl|sicherheit|s(é|e)curit|seguridad|seguran|sicurezza|beveiliging/i.test(words(el));}
var CARD=/\b(card|cards|cardnumber|ccnum|ccnumber|creditcard|debitcard|cvv2?|cvc2?|csc|cardholder|karte|kreditkarte|karteninhaber|pr(ü|u)fnummer|carte|cryptogramme|tarjeta|cart(ã|a)o|carta|kaarthouder|pasnummer)\b|kartennummer|kaartnummer/i;
var PAYF=/(^|\.)(stripe\.com|adyen\.com|braintreegateway\.com|paypal\.com|checkout\.com|mollie\.com|squareup\.com|klarna\.com|worldpay\.com)$/i;
function payFrame(f){try{return PAYF.test(new URL(f.getAttribute('src'),location.href).hostname);}catch(e){return false;}}
function cardField(el){var ac=(el.getAttribute('autocomplete')||'').toLowerCase();return /(^|\s)cc-/.test(ac)||CARD.test(words(el));}
function takesCard(root){var ins=qa('input,select',root);for(var i=0;i<ins.length;i++){if(itype(ins[i])!=='hidden'&&cardField(ins[i]))return true;}var fr=qa('iframe[src]',root);for(var j=0;j<fr.length;j++){if(payFrame(fr[j]))return true;}return false;}
var PRICE=/[£€$¥₹₩₽₺₪¢₱₫฿]\s?\d|\d\s?[£€$¥₹₩₽₺₪¢₱₫฿]|\b(usd|eur|gbp|chf|jpy|cad|aud|sek|nok|dkk|pln|brl|mxn|kr|zł)\s?\d|\d\s?(usd|eur|gbp|chf|kr|zł)\b/i;
function inSearch(el){var h=(el.getAttribute('enterkeyhint')||'').toLowerCase(),m=(el.getAttribute('inputmode')||'').toLowerCase();return h==='search'||m==='search'||!!up(el,'[role=search],search');}
function submitBtn(f){return f?f.querySelector('button:not([type=button]):not([type=reset]),input[type=submit],input[type=image]'):null;}
function formsInfo(){return Fs().map(function(f,i){var b=submitBtn(f);return {form:i,password:!!q1('input[type=password]',f),action:f.action||'',method:(f.getAttribute('method')||'get').toLowerCase(),submit:b?nameOf(b):'',payment:takesCard(f),price:PRICE.test(sq(f.innerText).slice(0,4000))};});}
function optEls(el){var ids=((el.getAttribute('aria-controls')||'')+' '+(el.getAttribute('aria-owns')||'')).split(/\s+/).filter(Boolean),rs=[el],out=[];
for(var i=0;i<ids.length;i++){var x=byId(el,ids[i]);if(x&&rs.indexOf(x)<0)rs.push(x);}
for(var k=0;k<rs.length;k++){var os=rs[k].querySelectorAll('[role=option]');for(var j=0;j<os.length&&out.length<200;j++){if(out.indexOf(os[j])<0)out.push(os[j]);}}return out;}
function optsOf(el){var out=[],os=optEls(el);for(var i=0;i<os.length&&out.length<20;i++){var t=sq(os[i].textContent);if(t&&out.indexOf(t)<0)out.push(t);}return out;}
function selOpt(el){var os=optEls(el);for(var i=0;i<os.length;i++){if(os[i].getAttribute('aria-selected')==='true')return os[i];}return null;}
function group(el){if(!el.name)return [el];var all=(el.form||el.getRootNode()).querySelectorAll('input[type=radio]'),out=[];for(var i=0;i<all.length;i++){if(all[i].name===el.name&&(all[i].form||null)===(el.form||null))out.push(all[i]);}return out;}
function desc(el){var t=tag(el),ty=itype(el),m=secret(el),ed=el.isContentEditable&&t!=='input'&&t!=='textarea';
var d={ref:refOf(el),role:roleOf(el),name:nameOf(el).slice(0,120),type:ty,autocomplete:el.getAttribute('autocomplete')||'',field:el.getAttribute('name')||'',id:el.id||'',label:labelOf(el),placeholder:el.getAttribute('placeholder')||'',href:t==='a'?(el.href||''):'',disabled:!!el.disabled||el.getAttribute('aria-disabled')==='true',form:el.form?Fs().indexOf(el.form):-1,submit:(t==='button'&&(el.getAttribute('type')||'submit').toLowerCase()==='submit'&&!!el.form)||(t==='input'&&(ty==='submit'||ty==='image')),masked:m,value:'',options:[],search:inSearch(el),editable:ed,closed:!el.shadowRoot&&t.indexOf('-')>0&&!!(W.customElements&&customElements.get(t))&&!el.children.length&&!sq(el.textContent)};
if(ty==='checkbox'||ty==='radio'||d.role==='checkbox'||d.role==='radio'||d.role==='switch'){d.checked=ty?!!el.checked:el.getAttribute('aria-checked')==='true';}
else if(t==='select'){var o=el.options[el.selectedIndex];d.value=o?sq(o.text):'';for(var i=0;i<el.options.length&&i<20;i++)d.options.push(sq(el.options[i].text));}
else if(!m&&(t==='input'||t==='textarea')&&['submit','button','reset','image'].indexOf(ty)<0){d.value=String(el.value||'').slice(0,200);}
else if(!m&&el.isContentEditable){d.value=sq(el.innerText).slice(0,200);}
if(t!=='select'&&(d.role==='combobox'||d.role==='listbox')){d.options=optsOf(el);if(!m&&t!=='input'&&t!=='textarea'&&!ed){var so=selOpt(el);d.value=(so?sq(so.textContent):sq(el.innerText||el.textContent)).slice(0,200);}}
if(ty==='radio'){var gr=group(el);if(gr.length>1){for(var gi=0;gi<gr.length&&gi<20;gi++){var gl=labelOf(gr[gi])||sq(gr[gi].value);if(gl)d.options.push(gl);}}}
return d;}
var SEL='a[href],button,input:not([type=hidden]),select,textarea,summary,[role=button],[role=link],[role=checkbox],[role=radio],[role=tab],[role=menuitem],[role=switch],[role=combobox],[role=textbox],[role=searchbox],[role=option],[contenteditable=""],[contenteditable=true]';
function scroller(){var se=D.scrollingElement||D.documentElement;if(!se||se.scrollHeight>W.innerHeight+2)return null;var best=null,area=0,all=D.body?qa('*',D.body,6000):[];for(var i=0;i<all.length&&i<6000;i++){var e=all[i];if(e.scrollHeight>e.clientHeight+2){var o=getComputedStyle(e).overflowY;if(o==='auto'||o==='scroll'){var ar=e.clientWidth*e.clientHeight;if(ar>area){area=ar;best=e;}}}}return best;}
function scrollAt(){var de=D.documentElement,sc=scroller();if(sc)return {x:Math.round(W.scrollX),y:Math.round(sc.scrollTop),w:Math.round(W.innerWidth),h:Math.round(sc.clientHeight),max:Math.max(0,Math.round(sc.scrollHeight-sc.clientHeight)),dpr:W.devicePixelRatio||1};
return {x:Math.round(W.scrollX),y:Math.round(W.scrollY),w:Math.round(W.innerWidth),h:Math.round(W.innerHeight),max:Math.max(0,Math.round((de?de.scrollHeight:0)-W.innerHeight)),dpr:W.devicePixelRatio||1};}
function hText(h){var s=sq(h.innerText);if(!s&&deep()&&(h.shadowRoot||h.querySelector('slot')))s=sq(flat(h));return s;}
function inView(){var out=[],hs=qa('h1,h2,h3,h4,[role=heading]');for(var i=0;i<hs.length&&out.length<8;i++){var r=hs[i].getBoundingClientRect();if(r.bottom>0&&r.top<W.innerHeight&&vis(hs[i])){var t=hText(hs[i]);if(t)out.push(t.slice(0,100));}}return out;}
function heads(){return [].slice.call(qa('h1,h2,h3',D,30)).slice(0,30).map(hText).filter(Boolean);}
var OVQ='[role=dialog],[role=alertdialog],[role=menu],[role=listbox],[role=tooltip],[role=tree],dialog,[aria-modal=true],[popover]';
function ovKind(el){if(!el||el.nodeType!==1)return '';var r=(el.getAttribute('role')||'').toLowerCase(),t=tag(el);
if(r==='dialog'||r==='alertdialog'||t==='dialog'||el.getAttribute('aria-modal')==='true')return 'dialog';if(r==='menu')return 'menu';if(r==='listbox'||r==='tree')return 'list';if(r==='tooltip'||el.hasAttribute('popover'))return 'popup';
var s=getComputedStyle(el);if(s.position==='fixed'||s.position==='absolute'){var b=el.getBoundingClientRect();if(b.width>=40&&b.height>=20&&(s.position==='fixed'||s.zIndex!=='auto'))return 'popup';}return '';}
function overlays(){var out=[],all=qa(OVQ);for(var i=0;i<all.length&&i<400;i++){if(shown(all[i]))out.push(all[i]);}return out;}
function leave(el){var o={bubbles:true,cancelable:true,composed:true,view:W,clientX:1,clientY:1,relatedTarget:D.body},ol={bubbles:false,cancelable:false,view:W,clientX:1,clientY:1,relatedTarget:D.body},bo={bubbles:true,cancelable:true,composed:true,view:W,clientX:1,clientY:1,relatedTarget:el};
try{el.dispatchEvent(new PointerEvent('pointerout',o));}catch(e){}try{el.dispatchEvent(new PointerEvent('pointerleave',ol));}catch(e){}el.dispatchEvent(new MouseEvent('mouseout',o));el.dispatchEvent(new MouseEvent('mouseleave',ol));
var bd=D.body||D.documentElement;try{bd.dispatchEvent(new PointerEvent('pointerover',bo));}catch(e){}bd.dispatchEvent(new MouseEvent('mouseover',bo));try{bd.dispatchEvent(new PointerEvent('pointermove',bo));}catch(e){}bd.dispatchEvent(new MouseEvent('mousemove',bo));}
function find(){var el=R.els[a.ref];return el&&el.isConnected?el:null;}
function same(el){var d=desc(el);return d.role===a.role&&d.name===a.name;}
function fire(el,k){el.dispatchEvent(new Event(k,{bubbles:true,composed:k==='input'}));}
function selIn(el){var rn=el.getRootNode();return rn!==D&&rn.getSelection?rn.getSelection():W.getSelection();}
function base(el){var b={password:!!q1('input[type=password]'),payment:takesCard(D),title:sq(D.title),headings:heads(),url:location.href,refs:R.els.length-1};if(!el){b.el=null;return b;}b.el=desc(el);b.form=el.form?formsInfo()[Fs().indexOf(el.form)]:null;return b;}
if(op==='pos')return {r:'ok',scroll:scrollAt(),inView:inView()};
if(op==='blocks'){var bl=qa('h1,h2,h3,h4,h5,h6,[role=heading],p,li,td,th,dt,dd,caption,figcaption,blockquote,pre,summary,label,legend,a,button,option'),bs=[],tot=0;
for(var i=0;i<bl.length&&bs.length<a.max;i++){var e=bl[i];if(!vis(e))continue;var t=textOf(e);if(!t)continue;t=t.slice(0,a.chars);tot+=t.length;if(tot>a.total)break;bs.push({ref:refOf(e),tag:tag(e),role:(e.getAttribute('role')||'').toLowerCase(),text:t});}
return {r:'ok',blocks:bs};}
if(op==='at'){var hit=ptAt(Number(a.x),Number(a.y));return base(hit?(up(hit,SEL)||hit):null);}
if(op==='active'){var ae=focusIn();return base(ae&&ae!==D.body&&ae!==D.documentElement?ae:null);}
if(op==='watch'){var ow=W.__yapWatch;if(ow&&ow.o){try{ow.o.disconnect();}catch(e){}}
var bf=overlays(),pp=R.pops||[];for(var pi=0;pi<pp.length;pi++){if(pp[pi].isConnected&&shown(pp[pi])&&bf.indexOf(pp[pi])<0)bf.push(pp[pi]);}
var w={recs:[],y:scrollAt().y,url:location.href,focus:focusIn(),before:bf,o:null,roots:new Set(),roots0:new Set(shs().trees),left:20000};
var OPT={childList:true,subtree:true,attributes:true,attributeFilter:['class','style','hidden','open','aria-hidden','aria-expanded'],characterData:true};
var see=function(r){if(!r||w.roots.has(r)||w.roots.size>=400)return false;w.roots.add(r);try{w.o.observe(r,OPT);}catch(e){}return true;};
var hunt=function(n){if(n.nodeType===1&&n.shadowRoot&&see(n.shadowRoot))hunt(n.shadowRoot);var all=n.querySelectorAll?n.querySelectorAll('*'):[];for(var k=0;k<all.length&&w.left>0;k++){w.left--;var s=all[k].shadowRoot;if(s&&see(s))hunt(s);}};
try{w.o=new MutationObserver(function(rs){for(var i=0;i<rs.length&&w.recs.length<3000;i++){var rc=rs[i];w.recs.push(rc);if(rc.type==='childList'){for(var j=0;j<rc.addedNodes.length&&w.left>0;j++){if(rc.addedNodes[j].nodeType===1)hunt(rc.addedNodes[j]);}}}});w.o.observe(D.documentElement,OPT);
var tr=shs().trees;for(var ti=1;ti<tr.length;ti++)see(tr[ti]);}catch(e){w.o=null;}
try{Object.defineProperty(W,'__yapWatch',{value:w,configurable:true,enumerable:false,writable:true});}catch(e){W.__yapWatch=w;}
return {r:'ok'};}
if(op==='changes'){var cw=W.__yapWatch,sa=scrollAt(),out={r:'ok',watched:false,scroll:sa,url:location.href,y0:sa.y,url0:location.href,opened:[],closed:[],inView:0,focus:null};
if(!cw)return out;var recs=cw.recs;if(cw.o){try{recs=recs.concat(cw.o.takeRecords());cw.o.disconnect();}catch(e){}}try{W.__yapWatch=null;}catch(e){}
out.watched=true;out.y0=cw.y;out.url0=cw.url;var fe=focusIn();if(fe&&fe!==cw.focus&&fe!==D.body&&fe!==D.documentElement)out.focus=desc(fe);
var els=[],had=new Set();function add(e){if(e&&e.nodeType===1&&!had.has(e)){had.add(e);els.push(e);}}
for(var i=0;i<recs.length;i++){var rc=recs[i];if(rc.type==='childList'){for(var j=0;j<rc.addedNodes.length;j++){var n=rc.addedNodes[j];if(n.nodeType===1){add(n);var inner=qa(OVQ,n,50);for(var q=0;q<inner.length&&q<50;q++)add(inner[q]);}else if(n.parentElement)add(n.parentElement);}}else if(rc.type==='attributes')add(rc.target);else if(rc.type==='characterData')add(rc.target.parentElement);}
var r0=cw.roots0||new Set(),trs=shs().trees;for(var tj=1;tj<trs.length;tj++){if(r0.has(trs[tj]))continue;add(trs[tj].host);var io=qa(OVQ,trs[tj],50);for(var q3=0;q3<io.length&&q3<50;q3++)add(io[q3]);}
var before=new Set(cw.before),now=overlays(),opened=[];
function within(e){for(var k=0;k<opened.length;k++){if(opened[k]!==e&&(opened[k].contains(e)||e.contains(opened[k])))return true;}return false;}
for(var i2=0;i2<now.length;i2++){if(!before.has(now[i2]))opened.push(now[i2]);}
for(var i3=0;i3<els.length&&opened.length<6;i3++){var e3=els[i3];if(!e3.isConnected||before.has(e3)||opened.indexOf(e3)>=0)continue;if(shown(e3)&&ovKind(e3)&&!within(e3))opened.push(e3);}
var kept=[];for(var i4=0;i4<opened.length&&kept.length<3;i4++){var e4=opened[i4],inside=false;for(var k2=0;k2<kept.length;k2++){if(kept[k2].contains(e4))inside=true;}if(!inside)kept.push(e4);}
out.opened=kept.map(function(e){return {kind:ovKind(e)||'popup',text:(deep()?sq(flat(e)):sq(e.innerText||e.textContent)).slice(0,160)};});
var still=[];for(var i7=0;i7<cw.before.length;i7++){if(shown(cw.before[i7])&&kept.indexOf(cw.before[i7])<0)still.push(cw.before[i7]);}R.pops=still.concat(kept).slice(-10);
for(var i5=0;i5<cw.before.length&&out.closed.length<3;i5++){var e5=cw.before[i5];if(!shown(e5))out.closed.push({kind:ovKind(e5)||'popup',text:''});}
var nv=0;for(var i6=0;i6<els.length&&nv<999;i6++){if(els[i6].isConnected&&shown(els[i6]))nv++;}out.inView=nv;
return out;}
if(op==='clickAt'){var el=find();if(!el)return {r:'gone'};if(!same(el))return {r:'changed'};if(el.disabled)return {r:'disabled'};var hit=ptAt(Number(a.x),Number(a.y));if(!hit||!holds(el,hit))return {r:'changed'};
var o={bubbles:true,cancelable:true,composed:true,clientX:Number(a.x),clientY:Number(a.y),view:W,button:0};try{hit.dispatchEvent(new PointerEvent('pointerdown',o));}catch(e){}hit.dispatchEvent(new MouseEvent('mousedown',o));try{el.focus({preventScroll:true});}catch(e){}try{hit.dispatchEvent(new PointerEvent('pointerup',o));}catch(e){}hit.dispatchEvent(new MouseEvent('mouseup',o));
var tg=tag(el)==='a'?(el.getAttribute('target')||'').toLowerCase():'';if(tg&&tg!=='_self'&&tg!=='_parent'&&tg!=='_top'&&/^https?:/i.test(el.href)){location.assign(el.href);return {r:'ok',went:true};}
hit.dispatchEvent(new MouseEvent('click',o));return {r:'ok'};}
if(op==='hover'){var el=find();if(!el)return {r:'gone'};if(!same(el))return {r:'changed'};if(R.hov&&R.hov!==el&&R.hov.isConnected)leave(R.hov);R.hov=el;into(el);var br=el.getBoundingClientRect(),o={bubbles:true,cancelable:true,composed:true,clientX:br.left+br.width/2,clientY:br.top+br.height/2,view:W};
['pointerover','pointerenter','mouseover','mouseenter','pointermove','mousemove'].forEach(function(k){try{el.dispatchEvent(k.indexOf('pointer')===0?new PointerEvent(k,o):new MouseEvent(k,o));}catch(e){}});return {r:'ok'};}
if(op==='check'){var el=find();if(!el)return {r:'gone'};if(!same(el))return {r:'changed'};if(el.disabled)return {r:'disabled'};var isOn=function(){return itype(el)?!!el.checked:el.getAttribute('aria-checked')==='true';};if(isOn()!==!!a.on){into(el);el.click();}return {r:'ok',checked:isOn()};}
if(op==='key'){var t0=a.ref?find():null;if(a.ref){if(!t0)return {r:'gone'};if(!same(t0))return {r:'changed'};into(t0);try{t0.focus({preventScroll:true});}catch(e){}}var kd=[];
for(var ki=0;ki<a.keys.length;ki++){var k=a.keys[ki],cur=focusIn()||D.body;
if(k==='Tab'||k==='Shift+Tab'){var fs=[].slice.call(qa('a[href],button,input:not([type=hidden]),select,textarea,[tabindex],[contenteditable=""],[contenteditable=true]')).filter(function(e){return vis(e)&&!e.disabled&&e.tabIndex>=0;});var j=fs.indexOf(cur);var nx=fs[k==='Tab'?j+1:(j<0?fs.length-1:j-1)];if(nx){into(nx);try{nx.focus({preventScroll:true});}catch(e){}}kd.push(k);continue;}
var kc=({Enter:13,Escape:27,Space:32,ArrowUp:38,ArrowDown:40,ArrowLeft:37,ArrowRight:39,PageUp:33,PageDown:34,Home:36,End:35})[k]||0;
var ko={key:k==='Space'?' ':k,code:k==='Space'?'Space':k,keyCode:kc,which:kc,bubbles:true,cancelable:true,composed:true};
var go=cur.dispatchEvent(new KeyboardEvent('keydown',ko));
if(go){var ct=tag(cur),cr=roleOf(cur);
if(k==='Enter'){if(cur.form&&ct==='input'){if(cur.form.requestSubmit)cur.form.requestSubmit();else cur.form.submit();}else if(ct==='a'||ct==='button'||cr==='button'||cr==='link'){cur.click();}else{cur.dispatchEvent(new KeyboardEvent('keypress',ko));}}
else if(k==='Space'&&(ct==='button'||itype(cur)==='checkbox'||itype(cur)==='radio'||cr==='button')){cur.click();}
else if(ct==='select'&&(k==='ArrowDown'||k==='ArrowUp')){var si=cur.selectedIndex+(k==='ArrowDown'?1:-1);if(si>=0&&si<cur.options.length){cur.selectedIndex=si;fire(cur,'input');fire(cur,'change');}}
else if(cur===D.body||cur===D.documentElement||ct==='a'){var sc=scroller(),sh=Math.round((sc?sc.clientHeight:W.innerHeight)*0.85),by=({PageDown:sh,Space:sh,PageUp:-sh,ArrowDown:40,ArrowUp:-40})[k];if(by)sBy(sc,by);else if(k==='Home')sTo(sc,0);else if(k==='End')sTo(sc,sc?sc.scrollHeight:D.documentElement.scrollHeight);}}
cur.dispatchEvent(new KeyboardEvent('keyup',ko));
if(k==='Escape'&&R.hov){if(R.hov.isConnected)leave(R.hov);R.hov=null;}
kd.push(k);}
var fe=focusIn();return {r:'ok',keys:kd,focused:fe&&fe!==D.body&&fe!==D.documentElement?desc(fe):null,scroll:scrollAt()};}
if(op==='snap'){var main=mainEl();var all=qa(SEL),els=[],more=false;
for(var i=0;i<all.length;i++){if(!vis(all[i]))continue;if(els.length>=a.max){more=true;break;}els.push(desc(all[i]));}
return {title:sq(D.title),url:location.href,text:main&&a.text?sq(shownText(main)).slice(0,a.text):'',headings:heads(),elements:els,forms:formsInfo(),password:!!q1('input[type=password]'),payment:takesCard(D),more:more,scroll:scrollAt()};}
if(op==='describe')return base(find());
if(op==='where'){var wh=find();if(!wh)return {r:'gone'};return {r:'ok',scroll:scrollAt(),inView:inView(),seen:shown(wh)};}
if(op==='scroll'){if(a.ref){var s=find();if(!s)return {r:'gone'};into(s);return {r:'ok',scroll:scrollAt(),inView:inView(),seen:shown(s)};}
var sc=scroller(),d=a.to||'down',n=Math.max(0.1,Math.min(10,Number(a.amount)||1)),step=Math.round((sc?sc.clientHeight:W.innerHeight)*0.85*n);
if(d==='top')sTo(sc,0);else if(d==='bottom')sTo(sc,sc?sc.scrollHeight:D.documentElement.scrollHeight);else sBy(sc,(d==='up'?-1:1)*step);
return {r:'ok',scroll:scrollAt(),inView:inView()};}
var el=find();if(!el)return {r:'gone'};if(!same(el))return {r:'changed'};if(el.disabled)return {r:'disabled'};
var caretIn=(function(){var s=selIn(el);return !!(s&&s.rangeCount&&el.contains(s.anchorNode));})();
into(el);try{el.focus({preventScroll:true});}catch(e){}
if(op==='click'){var tg=tag(el)==='a'?(el.getAttribute('target')||'').toLowerCase():'';
if(tg&&tg!=='_self'&&tg!=='_parent'&&tg!=='_top'&&/^https?:/i.test(el.href)){location.assign(el.href);return {r:'ok',went:true};}
el.click();return {r:'ok'};}
if(op==='choose'){var want=String(a.text).replace(/\s+/g,' ').trim().toLowerCase(),ct2=tag(el),cty=itype(el);
var pk=function(list,txt){var i;for(i=0;i<list.length;i++){if(txt(list[i]).toLowerCase()===want)return list[i];}for(i=0;i<list.length;i++){if(txt(list[i]).toLowerCase().indexOf(want)>=0)return list[i];}return null;};
if(ct2==='select'){var op2=[].slice.call(el.options),po2=pk(op2,function(x){return sq(x.text);})||pk(op2,function(x){return String(x.value);});if(!po2)return {r:'no-option'};el.value=po2.value;fire(el,'input');fire(el,'change');return {r:'ok',value:sq(po2.text)};}
if(cty==='radio'){var rh=pk(group(el),function(x){return labelOf(x)||sq(x.value);});if(!rh)return {r:'no-option'};if(rh.disabled)return {r:'disabled'};into(rh);if(!rh.checked)rh.click();return {r:'ok',value:labelOf(rh)||sq(rh.value),checked:!!rh.checked};}
var shut=function(){var ek={key:'Escape',code:'Escape',keyCode:27,which:27,bubbles:true,cancelable:true,composed:true};el.dispatchEvent(new KeyboardEvent('keydown',ek));el.dispatchEvent(new KeyboardEvent('keyup',ek));};
var oe=pk(optEls(el),function(x){return sq(x.textContent);});
if((!oe||!vis(oe))&&!a.again){el.click();oe=pk(optEls(el),function(x){return sq(x.textContent);});if((!oe&&!optEls(el).length)||(oe&&!vis(oe)))return {r:'opening'};}
if(!oe){var some=optsOf(el);shut();return {r:'no-option',options:some};}
into(oe);var po={bubbles:true,cancelable:true,composed:true,view:W,button:0};try{oe.dispatchEvent(new PointerEvent('pointerdown',po));}catch(e){}oe.dispatchEvent(new MouseEvent('mousedown',po));try{oe.dispatchEvent(new PointerEvent('pointerup',po));}catch(e){}oe.dispatchEvent(new MouseEvent('mouseup',po));oe.click();
var lb=oe.closest('[role=listbox]');if(lb&&shown(lb))shut();
var so2=selOpt(el);return {r:'ok',value:so2?sq(so2.textContent):sq(oe.textContent)};}
if(op==='type'){var t=tag(el);
if(t==='select'){var want=String(a.text).toLowerCase().trim(),pick=null;for(var i=0;i<el.options.length;i++){var o=el.options[i];if(sq(o.text).toLowerCase()===want||String(o.value).toLowerCase()===want){pick=o;break;}}
if(!pick){for(var j=0;j<el.options.length;j++){if(sq(el.options[j].text).toLowerCase().indexOf(want)>=0){pick=el.options[j];break;}}}if(!pick)return {r:'no-option'};el.value=pick.value;fire(el,'input');fire(el,'change');}
else if(t==='input'||t==='textarea'){var proto=t==='textarea'?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;var set=Object.getOwnPropertyDescriptor(proto,'value').set;set.call(el,String(a.text));fire(el,'input');fire(el,'change');}
else if(el.isContentEditable){var sel=selIn(el),txt=String(a.text);
if(txt){if(!caretIn||!sel.rangeCount){var rg=D.createRange();rg.selectNodeContents(el);rg.collapse(false);sel.removeAllRanges();sel.addRange(rg);var had=el.innerText||'';if(had.trim()&&!/\s$/.test(had))txt=' '+txt;}
var done=false;try{done=D.execCommand('insertText',false,txt);}catch(e){}
if(!done){var r2=sel.getRangeAt(0);r2.insertNode(D.createTextNode(txt));r2.collapse(false);fire(el,'input');}}}
else return {r:'not-a-field'};
var ins=el.isContentEditable&&t!=='input'&&t!=='textarea';
var now=secret(el)?'':(t==='select'?sq(el.options[el.selectedIndex]?el.options[el.selectedIndex].text:''):(ins?sq(el.innerText):String(el.value||'')).slice(0,200));
if(a.submit){if(el.form){if(el.form.requestSubmit)el.form.requestSubmit();else el.form.submit();}else{['keydown','keypress','keyup'].forEach(function(k){el.dispatchEvent(new KeyboardEvent(k,{key:'Enter',code:'Enter',keyCode:13,which:13,bubbles:true,composed:true}));});}}
return {r:'ok',value:now,inserted:ins};}
return {r:'unknown'};
"#
);

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

/// Choose the option `text` names in element `r` (a select, a radio
/// button's group, a combobox or listbox — opened first when its options
/// aren't there; `again`: it was opened already), if it's still `role`
/// "`name`". Its answer: `ok` with the choice, `no-option`, or `opening`
/// (opened, its options not yet drawn: ask again).
pub fn choose(r: u32, role: &str, name: &str, text: &str, again: bool) -> String {
    build("choose", serde_json::json!({ "ref": r, "role": role, "name": name, "text": text, "again": again }))
}

/// Scroll the page `to` "up", "down" (`amount` screens), "top" or "bottom",
/// or element `r` into view (centred), instantly whatever the page's CSS
/// says. Its answer: where it is now and the headings in view (and, for an
/// element, whether it's in view: `seen`).
pub fn scroll(to: &str, amount: f64, r: Option<u32>) -> String {
    build("scroll", serde_json::json!({ "to": to, "amount": amount, "ref": r }))
}

/// Where the page is scrolled to, its view's size and the headings in view.
pub fn pos() -> String {
    build("pos", serde_json::json!({}))
}

/// Whether element `r` is in view now (`seen`), with where the page is.
pub fn where_is(r: u32) -> String {
    build("where", serde_json::json!({ "ref": r }))
}

/// Start watching the page for what an action changes (`changes` reads it).
pub fn watch() -> String {
    build("watch", serde_json::json!({}))
}

/// What changed since `watch` (`rules::Changes`), and stop watching.
pub fn changes() -> String {
    build("changes", serde_json::json!({}))
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

/// The pointer over element `r`, if it's still `role` "`name`" (the one
/// hovered before let go).
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
/// "`name`"), else in the one with the focus (the page itself when none
/// has it); Escape lets a hover go too.
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
        assert!(choose(5, "combobox", "Sort", "Edit date\"});alert(1);//", false).contains("})(\"choose\",{"));
        // A point click runs before the shared path (which scrolls the element into view: the point would move).
        assert!(LIB.find("op==='clickAt'").unwrap() < LIB.find("var caretIn").unwrap());
        // Never a secret's value: the library masks them by type, autocomplete and name.
        assert!(LIB.contains("current-password|new-password|one-time-code") && LIB.contains("masked:m"));
        // Every action checks the element is still the one described first.
        assert!(LIB.contains("if(!same(el))return {r:'changed'}"));
    }

    #[test]
    fn every_scroll_yap_makes_is_instant_whatever_the_pages_css() {
        // (8 Nov 2026: a page with scroll-behavior: smooth answered mid-scroll.)
        // No scroll by assignment or a bare scrollIntoView / scrollBy / scrollTo outside the instant helpers.
        let outside: String = LIB.lines().filter(|l| !l.starts_with("function sTo") && !l.starts_with("function sBy") && !l.starts_with("function into")).collect::<Vec<_>>().join("\n");
        for bare in ["scrollTop=", ".scrollIntoView(", ".scrollBy(", ".scrollTo("] {
            assert!(!outside.contains(bare), "{bare} outside the instant helpers");
        }
        assert!(LIB.contains("behavior:'instant'"));
        // A focus never scrolls on its own.
        assert!(!outside.contains(".focus()") && outside.contains("focus({preventScroll:true})"));
        // Escape and a new hover let the last hover go.
        assert!(LIB.contains("if(k==='Escape'&&R.hov)") && LIB.contains("if(R.hov&&R.hov!==el"));
    }
}
