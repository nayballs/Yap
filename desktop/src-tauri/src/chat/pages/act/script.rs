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
//!   how many elements changed in view, a frame that went somewhere new
//!   (`rules::Changes`).
//! - `press` (8 Nov 2026): a double or triple click (the words there
//!   selected, as a person's double-click selects a word — a script's
//!   events alone never select), a right-click (`contextmenu`: the page's own
//!   menu, if it has one) or a middle one (`auxclick`), with the pointer and
//!   mouse events a person's would send, each `button`, `buttons` and
//!   `detail` as theirs.
//! - `dragStart`, `dragMove`, `dragEnd` (8 Nov 2026): a drag in steps Rust
//!   paces (a page's drag library sets itself up between them) — the
//!   pointer down, moves and up, or, where what's under it is draggable,
//!   HTML5 drag and drop (`dragstart`, `dragenter` / `dragover` /
//!   `dragleave`, `drop` where the target takes it, `dragend`, one
//!   `DataTransfer` throughout); a slider follows the pointer; what moved
//!   (a list item's place before and after) is told back.
//!
//! **Frames**: an element in a same-origin frame is found, described and
//! acted on as any other (`deep_js!`): its events are made in its own window
//! with its own coordinates (`loc`: the view's point less the frame's
//! place), its field's value set by its own window's setter, its words typed
//! by its own document. A frame from another site is an element of its own
//! (`desc`: role `frame`, `foreign`, its address), never reached inside.
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
function vis(el){if(!el||!el.isConnected)return false;if(!el.getClientRects().length)return false;var s=cs(el);if(s.visibility==='hidden'||s.display==='none')return false;return !up(el,'[aria-hidden="true"],[hidden],[inert]');}
function shown(el){if(!vis(el))return false;var b=el.getBoundingClientRect(),w=win(el);if(!(b.width>0&&b.height>0&&b.bottom>0&&b.right>0&&b.top<w.innerHeight&&b.left<w.innerWidth))return false;if(w===W)return true;var o=fOff(el.ownerDocument);return b.bottom+o.y>0&&b.right+o.x>0&&b.top+o.y<W.innerHeight&&b.left+o.x<W.innerWidth;}
function loc(el,x,y){var o=fOff(el.ownerDocument);return {x:x-o.x,y:y-o.y};}
function mk(el,k,o){var w=win(el);o.view=w;var C=/^pointer/.test(k)?w.PointerEvent:/^key/.test(k)?w.KeyboardEvent:/^(drag|drop)/.test(k)?w.DragEvent:/^(mouse|click|dblclick|contextmenu|auxclick)/.test(k)?w.MouseEvent:w.Event;return new (C||w.Event)(k,o);}
function send(el,k,o){try{return el.dispatchEvent(mk(el,k,o));}catch(e){return true;}}
function pev(el,k,x,y,btn,bs,det){var lv=/(enter|leave)$/.test(k),o={bubbles:!lv,cancelable:!lv,composed:!lv,clientX:x,clientY:y,screenX:x,screenY:y,button:btn||0,buttons:bs||0,detail:det||0,ctrlKey:!!(a.mods&&a.mods.ctrl),shiftKey:!!(a.mods&&a.mods.shift),altKey:!!(a.mods&&a.mods.alt),metaKey:!!(a.mods&&a.mods.meta)};if(/^pointer/.test(k)){o.pointerId=1;o.pointerType='mouse';o.isPrimary=true;o.width=1;o.height=1;o.pressure=bs?0.5:0;}return send(el,k,o);}
function sTo(sc,y){try{if(sc)sc.scrollTo({top:y,behavior:'instant'});else W.scrollTo({left:W.scrollX,top:y,behavior:'instant'});}catch(e){if(sc)sc.scrollTop=y;else W.scrollTo(W.scrollX,y);}}
function sBy(sc,dy,dx){dx=dx||0;try{if(sc)sc.scrollBy({top:dy,left:dx,behavior:'instant'});else W.scrollBy({top:dy,left:dx,behavior:'instant'});}catch(e){if(sc){sc.scrollTop+=dy;sc.scrollLeft+=dx;}else W.scrollBy(dx,dy);}}
function into(el){try{var tall=false,wide=false,iw=win(el);try{var b0=el.getBoundingClientRect();tall=b0.height>iw.innerHeight*0.9;wide=b0.width>iw.innerWidth*0.9;}catch(e0){}el.scrollIntoView({block:tall?'start':'center',inline:wide?'start':'nearest',behavior:'instant'});if(tall){var b1=el.getBoundingClientRect();if(b1.top<8&&b1.top>-iw.innerHeight)iw.scrollBy({top:b1.top-8,left:0,behavior:'instant'});}}catch(e){try{el.scrollIntoView();}catch(e2){}}}
var F_=null;function Fs(){if(!F_)F_=[].slice.call(deep()?qa('form'):D.forms);return F_;}
function tag(el){return el.tagName.toLowerCase();}
function isField(el){var t=tag(el);return t==='input'||t==='textarea'||t==='select'||el.isContentEditable;}
function itype(el){return tag(el)==='input'?(el.getAttribute('type')||'text').toLowerCase():'';}
function roleOf(el){var r=(el.getAttribute('role')||'').toLowerCase();if(r)return r;var t=tag(el),ty=itype(el);
if(t==='a')return 'link';if(t==='button'||t==='summary')return 'button';if(t==='select')return 'combobox';if(t==='textarea')return 'textbox';
if(t==='input'){if(['submit','button','reset','image'].indexOf(ty)>=0)return 'button';if(ty==='checkbox')return 'checkbox';if(ty==='radio')return 'radio';if(ty==='search')return 'searchbox';if(ty==='range')return 'slider';if(ty==='number')return 'spinbutton';return 'textbox';}
if(el.isContentEditable)return 'textbox';return t;}
function labelOf(el){var out=[],f=isField(el);if(el.labels){for(var i=0;i<el.labels.length;i++)out.push(textOf(el.labels[i]));}if(!out.length){var l=el.closest('label')||(f?up(el,'label'):null);if(l)out.push(textOf(l));}
if(!out.length&&f){var rn0=el.getRootNode(),h=rn0&&rn0.host;if(h){var hl=sq(h.getAttribute('aria-label')||h.getAttribute('label'));if(hl)out.push(hl);}}return sq(out.join(' ')).slice(0,120);}
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
var d={ref:refOf(el),role:roleOf(el),name:nameOf(el).slice(0,120),type:ty,autocomplete:el.getAttribute('autocomplete')||'',field:el.getAttribute('name')||'',id:el.id||'',label:labelOf(el),placeholder:el.getAttribute('placeholder')||'',href:t==='a'?(el.href||''):'',disabled:!!el.disabled||el.getAttribute('aria-disabled')==='true',form:el.form?Fs().indexOf(el.form):-1,submit:(t==='button'&&(el.getAttribute('type')||'submit').toLowerCase()==='submit'&&!!el.form)||(t==='input'&&(ty==='submit'||ty==='image')),masked:m,value:'',options:[],search:inSearch(el),editable:ed,closed:!el.shadowRoot&&t.indexOf('-')>0&&!!(win(el).customElements&&win(el).customElements.get(t))&&!el.children.length&&!sq(el.textContent)};
if(t==='iframe'||t==='frame'){var fi=frameOf(el);d.role='frame';if(!d.name)d.name=fi.title;d.frame=fi.src;d.foreign=!fdoc(el);d.sandboxed=fi.sandboxed;}
if(ty==='checkbox'||ty==='radio'||d.role==='checkbox'||d.role==='radio'||d.role==='switch'){d.checked=ty?!!el.checked:el.getAttribute('aria-checked')==='true';}
else if(t==='select'){var o=el.options[el.selectedIndex];d.value=o?sq(o.text):'';for(var i=0;i<el.options.length&&i<20;i++)d.options.push(sq(el.options[i].text));}
else if(!m&&(t==='input'||t==='textarea')&&['submit','button','reset','image'].indexOf(ty)<0){d.value=String(el.value||'').slice(0,200);}
else if(!m&&el.isContentEditable){d.value=sq(el.innerText).slice(0,200);}
if(t!=='select'&&(d.role==='combobox'||d.role==='listbox')){d.options=optsOf(el);if(!m&&t!=='input'&&t!=='textarea'&&!ed){var so=selOpt(el);d.value=(so?sq(so.textContent):sq(el.innerText||el.textContent)).slice(0,200);}}
if(ty==='radio'){var gr=group(el);if(gr.length>1){for(var gi=0;gi<gr.length&&gi<20;gi++){var gl=labelOf(gr[gi])||sq(gr[gi].value);if(gl)d.options.push(gl);}}}
return d;}
var SEL='a[href],button,input:not([type=hidden]),select,textarea,summary,[role=button],[role=link],[role=checkbox],[role=radio],[role=tab],[role=menuitem],[role=switch],[role=combobox],[role=textbox],[role=searchbox],[role=option],[contenteditable=""],[contenteditable=true]';
function scroller(){var se=D.scrollingElement||D.documentElement;if(!se||se.scrollHeight>W.innerHeight+2)return null;var best=null,area=0,all=D.body?qa('*',D.body,6000):[];for(var i=0;i<all.length&&i<6000;i++){var e=all[i];if(e.ownerDocument!==D)continue;if(e.scrollHeight>e.clientHeight+2){var o=cs(e).overflowY;if(o==='auto'||o==='scroll'){var ar=e.clientWidth*e.clientHeight;if(ar>area){area=ar;best=e;}}}}return best;}
function scrollAt(){var de=D.documentElement,sc=scroller(),hx=sc&&sc.scrollWidth>sc.clientWidth+2,vp=(D.compatMode==='BackCompat'&&D.body)?D.body:de,cw=(vp&&vp.clientWidth)||W.innerWidth,chh=(vp&&vp.clientHeight)||W.innerHeight,mx=Math.max(0,Math.round(hx?sc.scrollWidth-sc.clientWidth:(de?de.scrollWidth:0)-cw));if(sc)return {x:Math.round(hx?sc.scrollLeft:W.scrollX),mx:mx,y:Math.round(sc.scrollTop),w:Math.round(W.innerWidth),h:Math.round(sc.clientHeight),max:Math.max(0,Math.round(sc.scrollHeight-sc.clientHeight)),dpr:W.devicePixelRatio||1};
return {x:Math.round(W.scrollX),mx:mx,y:Math.round(W.scrollY),w:Math.round(W.innerWidth),h:Math.round(W.innerHeight),max:Math.max(0,Math.round((de?de.scrollHeight:0)-chh)),dpr:W.devicePixelRatio||1};}
function hText(h){var s=sq(h.innerText);if(!s&&deep()&&(h.shadowRoot||h.querySelector('slot')))s=sq(flat(h));return s;}
function inView(){var out=[],hs=qa('h1,h2,h3,h4,[role=heading]');for(var i=0;i<hs.length&&out.length<8;i++){var r=rr(hs[i]);if(r.bottom>0&&r.top<W.innerHeight&&vis(hs[i])){var t=hText(hs[i]);if(t)out.push(t.slice(0,100));}}return out;}
function heads(){return [].slice.call(qa('h1,h2,h3',D,30)).slice(0,30).map(hText).filter(Boolean);}
var OVQ='[role=dialog],[role=alertdialog],[role=menu],[role=listbox],[role=tooltip],[role=tree],dialog,[aria-modal=true],[popover]';
function ovKind(el){if(!el||el.nodeType!==1)return '';var r=(el.getAttribute('role')||'').toLowerCase(),t=tag(el);
if(r==='dialog'||r==='alertdialog'||t==='dialog'||el.getAttribute('aria-modal')==='true')return 'dialog';if(r==='menu')return 'menu';if(r==='listbox'||r==='tree')return 'list';if(r==='tooltip'||el.hasAttribute('popover'))return 'popup';
var s=cs(el);if(s.position==='fixed'||s.position==='absolute'){var b=el.getBoundingClientRect();if(b.width>=40&&b.height>=20&&(s.position==='fixed'||s.zIndex!=='auto'))return 'popup';}return '';}
function overlays(){var out=[],all=qa(OVQ);for(var i=0;i<all.length&&i<400;i++){if(shown(all[i]))out.push(all[i]);}return out;}
function leave(el){var bd=el.ownerDocument.body||el.ownerDocument.documentElement,o={bubbles:true,cancelable:true,composed:true,clientX:1,clientY:1,relatedTarget:bd},ol={bubbles:false,cancelable:false,clientX:1,clientY:1,relatedTarget:bd},bo=function(){return {bubbles:true,cancelable:true,composed:true,clientX:1,clientY:1,relatedTarget:el};};
send(el,'pointerout',o);send(el,'pointerleave',ol);send(el,'mouseout',{bubbles:true,cancelable:true,composed:true,clientX:1,clientY:1,relatedTarget:bd});send(el,'mouseleave',{bubbles:false,cancelable:false,clientX:1,clientY:1,relatedTarget:bd});
send(bd,'pointerover',bo());send(bd,'mouseover',bo());send(bd,'pointermove',bo());send(bd,'mousemove',bo());}
function find(){var el=R.els[a.ref];return el&&el.isConnected?el:null;}
function same(el){var d=desc(el);return d.role===a.role&&d.name===a.name;}
function fire(el,k){send(el,k,{bubbles:true,composed:k==='input'});}
function words2(t,o){var re=/[\p{L}\p{N}_'’-]/u,s=o,e=o;if(!re.test(t[o]||'')&&o>0&&re.test(t[o-1]))s=e=o-1;while(s>0&&re.test(t[s-1]))s--;while(e<t.length&&re.test(t[e]))e++;return [s,e];}
function pickWords(t,lx,ly,n){var d=t.ownerDocument,w=win(t),tn=tag(t);
if((tn==='input'||tn==='textarea')&&!secret(t)){try{var v=String(t.value||'');if(n>=3)t.select();else{var se=words2(v,t.selectionStart||0);t.setSelectionRange(se[0],se[1]);}return v.slice(t.selectionStart,t.selectionEnd);}catch(x){return '';}}
var r=d.caretRangeFromPoint?d.caretRangeFromPoint(lx,ly):null;if(!r)return '';var sel=w.getSelection(),nd=r.startContainer,rg=d.createRange();
if(n>=3){var bl=nd.nodeType===3?nd.parentElement:nd,b=bl&&bl.closest?bl.closest('p,li,td,th,h1,h2,h3,h4,h5,h6,dd,dt,blockquote,pre,figcaption,caption,label,div'):null;if(!b)return '';rg.selectNodeContents(b);}
else{if(nd.nodeType!==3)return '';var be=words2(nd.data,r.startOffset);if(be[1]<=be[0])return '';rg.setStart(nd,be[0]);rg.setEnd(nd,be[1]);}
sel.removeAllRanges();sel.addRange(rg);return sq(sel.toString());}
function selNow(t){var s=win(t).getSelection(),x=s?sq(s.toString()):'';if(!x&&(tag(t)==='input'||tag(t)==='textarea')&&!secret(t)){try{x=String(t.value||'').slice(t.selectionStart,t.selectionEnd);}catch(e){}}return x.slice(0,200);}
function itemOf(el){var it=up(el,'li,tr,[role=listitem],[role=row],[role=option],[role=treeitem]');if(it)return it;for(var e=el,n=0;e&&n<40;n++){if(e.nodeType===1&&e.draggable===true&&tag(e)!=='a'&&tag(e)!=='img')return e;e=e.parentElement||hostOf(e);}return el;}
function kidsOf(p){var out=[];if(!p)return out;for(var c=p.firstElementChild;c&&out.length<300;c=c.nextElementSibling){if(vis(c))out.push(c);}return out;}
function placeOf(p){for(var e=p,n=0;e&&n<8;n++){if(e.getAttribute){var l=sq(e.getAttribute('aria-label'))||lbl(e);if(l)return l.slice(0,60);var h=e.querySelector(':scope > h1,:scope > h2,:scope > h3,:scope > h4,:scope > h5,:scope > header,:scope > legend,:scope > caption');if(h){var ht=textOf(h);if(ht)return ht.slice(0,60);}}e=e.parentElement||hostOf(e);}return '';}
function dragOf(e){for(var n=0;e&&n<40;n++){if(e.nodeType===1&&e.draggable===true)return e;e=e.parentElement||hostOf(e);}return null;}
function dev(el,k,p,dt){var l=loc(el,p.x,p.y),cn=k!=='dragleave'&&k!=='dragend';return send(el,k,{bubbles:true,cancelable:cn,composed:true,clientX:l.x,clientY:l.y,screenX:l.x,screenY:l.y,dataTransfer:dt,buttons:(k==='drop'||k==='dragend')?0:1});}
function setRange(el,p,end){var b=rr(el);if(!b.width)return;var f=Math.max(0,Math.min(1,(p.x-b.left)/b.width)),mn=el.min===''?0:+el.min,mx=el.max===''?100:+el.max,st=el.step==='any'?0:(+el.step||1),v=mn+(mx-mn)*f;if(st)v=Math.round((v-mn)/st)*st+mn;v=Math.max(mn,Math.min(mx,v));
var set=Object.getOwnPropertyDescriptor(win(el).HTMLInputElement.prototype,'value').set;set.call(el,String(+v.toFixed(6)));fire(el,'input');if(end)fire(el,'change');}
function selIn(el){var rn=el.getRootNode();return rn!==D&&rn.getSelection?rn.getSelection():W.getSelection();}
function base(el){var b={password:!!q1('input[type=password]'),payment:takesCard(D),title:sq(D.title),headings:heads(),url:location.href,refs:R.els.length-1};if(!el){b.el=null;return b;}b.el=desc(el);b.form=el.form?formsInfo()[Fs().indexOf(el.form)]:null;return b;}
if(op==='pos')return {r:'ok',scroll:scrollAt(),inView:inView()};
if(op==='blocks'){var bl=qa('h1,h2,h3,h4,h5,h6,[role=heading],p,li,td,th,dt,dd,caption,figcaption,blockquote,pre,summary,label,legend,a,button,option'),bs=[],tot=0;
for(var i=0;i<bl.length&&bs.length<a.max;i++){var e=bl[i];if(!vis(e))continue;var t=textOf(e);if(!t)continue;t=t.slice(0,a.chars);tot+=t.length;if(tot>a.total)break;bs.push({ref:refOf(e),tag:tag(e),role:(e.getAttribute('role')||'').toLowerCase(),text:t});}
return {r:'ok',blocks:bs};}
if(op==='at'){var hit=ptAt(Number(a.x),Number(a.y));return base(hit?(up(hit,SEL)||hit):null);}
if(op==='active'){var ae=focusIn();return base(bare(ae)?null:ae);}
if(op==='watch'){var ow=W.__yapWatch;if(ow&&ow.o){try{ow.o.disconnect();}catch(e){}}
var bf=overlays(),pp=R.pops||[];for(var pi=0;pi<pp.length;pi++){if(pp[pi].isConnected&&shown(pp[pi])&&bf.indexOf(pp[pi])<0)bf.push(pp[pi]);}
var fl=[];shs().inner.forEach(function(d,h){if(d.nodeType===9&&fl.length<40){try{fl.push([h,d.defaultView.location.href]);}catch(e){}}});
var w={recs:[],y:scrollAt().y,url:location.href,focus:focusIn(),before:bf,o:null,roots:new Set(),roots0:new Set(shs().trees),left:20000,fl:fl};
var OPT={childList:true,subtree:true,attributes:true,attributeFilter:['class','style','hidden','open','aria-hidden','aria-expanded'],characterData:true};
var see=function(r){if(!r||w.roots.has(r)||w.roots.size>=400)return false;w.roots.add(r);try{w.o.observe(r,OPT);}catch(e){}return true;};
var hunt=function(n){if(n.nodeType===1&&n.shadowRoot&&see(n.shadowRoot))hunt(n.shadowRoot);var all=n.querySelectorAll?n.querySelectorAll('*'):[];for(var k=0;k<all.length&&w.left>0;k++){w.left--;var s=all[k].shadowRoot;if(s&&see(s))hunt(s);}};
try{w.o=new MutationObserver(function(rs){for(var i=0;i<rs.length&&w.recs.length<3000;i++){var rc=rs[i];w.recs.push(rc);if(rc.type==='childList'){for(var j=0;j<rc.addedNodes.length&&w.left>0;j++){if(rc.addedNodes[j].nodeType===1)hunt(rc.addedNodes[j]);}}}});w.o.observe(D.documentElement,OPT);
var tr=shs().trees;for(var ti=1;ti<tr.length;ti++)see(tr[ti]);}catch(e){w.o=null;}
try{Object.defineProperty(W,'__yapWatch',{value:w,configurable:true,enumerable:false,writable:true});}catch(e){W.__yapWatch=w;}
return {r:'ok'};}
if(op==='changes'){var cw=W.__yapWatch,sa=scrollAt(),out={r:'ok',watched:false,scroll:sa,url:location.href,y0:sa.y,url0:location.href,opened:[],closed:[],inView:0,focus:null,frames:[]};
if(!cw)return out;var recs=cw.recs;if(cw.o){try{recs=recs.concat(cw.o.takeRecords());cw.o.disconnect();}catch(e){}}try{W.__yapWatch=null;}catch(e){}
out.watched=true;out.y0=cw.y;out.url0=cw.url;var fe=focusIn();if(fe&&fe!==cw.focus&&!bare(fe))out.focus=desc(fe);
(cw.fl||[]).forEach(function(p){if(out.frames.length>=3||!p[0].isConnected)return;var fd=fdoc(p[0]),nw='';try{nw=fd?fd.defaultView.location.href:'';}catch(e){}if(!nw)nw=p[0].src||'';if(nw&&nw!==p[1]&&!/^about:/.test(nw))out.frames.push(nw);});
var els=[],had=new Set();function add(e){if(e&&e.nodeType===1&&!had.has(e)){had.add(e);els.push(e);}}
for(var i=0;i<recs.length;i++){var rc=recs[i];if(rc.type==='childList'){for(var j=0;j<rc.addedNodes.length;j++){var n=rc.addedNodes[j];if(n.nodeType===1){add(n);var inn=qa(OVQ,n,50);for(var q=0;q<inn.length&&q<50;q++)add(inn[q]);}else if(n.parentElement)add(n.parentElement);}}else if(rc.type==='attributes')add(rc.target);else if(rc.type==='characterData')add(rc.target.parentElement);}
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
var p=loc(hit,Number(a.x),Number(a.y));pev(hit,'pointerdown',p.x,p.y,0,1);pev(hit,'mousedown',p.x,p.y,0,1,1);try{el.focus({preventScroll:true});}catch(e){}pev(hit,'pointerup',p.x,p.y,0,0);pev(hit,'mouseup',p.x,p.y,0,0,1);
var tg=tag(el)==='a'?(el.getAttribute('target')||'').toLowerCase():'';if(tg&&tg!=='_self'&&tg!=='_parent'&&tg!=='_top'&&/^https?:/i.test(el.href)){location.assign(el.href);return {r:'ok',went:true};}
pev(hit,'click',p.x,p.y,0,0,1);return {r:'ok'};}
if(op==='press'){var el=find();if(!el)return {r:'gone'};if(!same(el))return {r:'changed'};if(el.disabled)return {r:'disabled'};var hit,px,py;
if(a.at){px=Number(a.x);py=Number(a.y);hit=ptAt(px,py);if(!hit||!holds(el,hit))return {r:'changed'};}
else{if(!shown(el))into(el);var c0=rr(el);px=c0.left+c0.width/2;py=c0.top+c0.height/2;hit=ptAt(px,py);if(!hit||!holds(el,hit))hit=el;}
var btn=a.button===1||a.button===2?a.button:0,n=btn===0?Math.max(1,Math.min(3,Number(a.count)||1)):1,bs=btn===0?1:btn===1?4:2,p=loc(hit,px,py),out={r:'ok'};
var tg=btn===0&&tag(el)==='a'?(el.getAttribute('target')||'').toLowerCase():'';if(tg&&tg!=='_self'&&tg!=='_parent'&&tg!=='_top'&&/^https?:/i.test(el.href)){location.assign(el.href);return {r:'ok',went:true};}
for(var i=1;i<=n;i++){pev(hit,'pointerdown',p.x,p.y,btn,bs);var dn=pev(hit,'mousedown',p.x,p.y,btn,bs,i);if(i===1&&btn===0){try{el.focus({preventScroll:true});}catch(e){}}if(btn===0&&i>=2&&dn)pickWords(hit,p.x,p.y,i);
pev(hit,'pointerup',p.x,p.y,btn,0);pev(hit,'mouseup',p.x,p.y,btn,0,i);if(btn===0){pev(hit,'click',p.x,p.y,0,0,i);if(i===2)pev(hit,'dblclick',p.x,p.y,0,0,2);}}
if(btn===2)out.menu=!pev(hit,'contextmenu',p.x,p.y,2,0,1);
if(btn===1)pev(hit,'auxclick',p.x,p.y,1,0,1);
if(n>1)out.selected=selNow(hit.isConnected?hit:el);
return out;}
if(op==='dragStart'){var el=find();if(!el)return {r:'gone'};if(!same(el))return {r:'changed'};if(el.disabled)return {r:'disabled'};var to=a.to||{},te=null;
if(to.ref){te=R.els[to.ref];if(!te||!te.isConnected)return {r:'gone'};var tdd=desc(te);if(tdd.role!==to.role||tdd.name!==to.name)return {r:'changed'};}
var p0,p1;if(a.at){p0={x:Number(a.x),y:Number(a.y)};var h0=ptAt(p0.x,p0.y);if(!h0||!holds(el,h0))return {r:'changed'};}else{if(!shown(el))into(el);var c1=rr(el);p0={x:c1.left+c1.width/2,y:c1.top+c1.height/2};}
if(to.at)p1={x:Number(to.x),y:Number(to.y)};else if(te){var c2=rr(te);p1={x:c2.left+c2.width/2,y:c2.top+c2.height/2};}else return {r:'gone'};
var src=ptAt(p0.x,p0.y);if(!src||!holds(el,src))src=el;var it=itemOf(el),ks=kidsOf(it.parentElement);
var g={el:el,src:src,p0:p0,p1:p1,last:src,dt:null,html5:false,it:it,list:it.parentElement,i0:ks.indexOf(it),txt:textOf(it).slice(0,80),range:tag(el)==='input'&&itype(el)==='range'};
var l0=loc(src,p0.x,p0.y);pev(src,'pointerover',l0.x,l0.y,0,0);pev(src,'pointerenter',l0.x,l0.y,0,0);pev(src,'mouseover',l0.x,l0.y,0,0);pev(src,'pointerdown',l0.x,l0.y,0,1);var dn=pev(src,'mousedown',l0.x,l0.y,0,1,1);
var dr=g.range?null:dragOf(src);if(dr&&dn){try{var dt=new (win(dr).DataTransfer)();if(tag(dr)==='a'&&dr.href){dt.setData('text/uri-list',dr.href);dt.setData('text/plain',dr.href);}else if(tag(dr)==='img'&&dr.src){dt.setData('text/uri-list',dr.src);}g.dt=dt;if(dev(dr,'dragstart',p0,dt)){g.html5=true;g.el=dr;pev(src,'pointercancel',l0.x,l0.y,0,0);}}catch(e){g.html5=false;}}
R.dg=g;return {r:'ok',html5:g.html5};}
if(op==='dragMove'){var g=R.dg;if(!g)return {r:'gone'};var f=Math.max(0,Math.min(1,Number(a.f)||0)),p={x:g.p0.x+(g.p1.x-g.p0.x)*f,y:g.p0.y+(g.p1.y-g.p0.y)*f},t=ptAt(p.x,p.y)||g.last;
if(g.html5){if(t!==g.last){dev(g.last,'dragleave',p,g.dt);dev(t,'dragenter',p,g.dt);}dev(g.el,'drag',p,g.dt);dev(t,'dragover',p,g.dt);}
else{var lp=loc(t,p.x,p.y);if(t!==g.last){var ll=loc(g.last,p.x,p.y);pev(g.last,'pointerout',ll.x,ll.y,0,1);pev(g.last,'mouseout',ll.x,ll.y,0,1);pev(t,'pointerover',lp.x,lp.y,0,1);pev(t,'mouseover',lp.x,lp.y,0,1);}pev(t,'pointermove',lp.x,lp.y,0,1);pev(t,'mousemove',lp.x,lp.y,0,1);if(g.range)setRange(g.el,p,false);}
g.last=t;return {r:'ok'};}
if(op==='dragEnd'){var g=R.dg;if(!g)return {r:'gone'};R.dg=null;var p=g.p1,t=ptAt(p.x,p.y)||g.last,out={r:'ok',html5:g.html5,dropped:false};
if(g.html5){if(t!==g.last){dev(g.last,'dragleave',p,g.dt);dev(t,'dragenter',p,g.dt);}var ok=!dev(t,'dragover',p,g.dt);if(ok){dev(t,'drop',p,g.dt);out.dropped=true;}else dev(t,'dragleave',p,g.dt);dev(g.el,'dragend',p,g.dt);}
else{var lp=loc(t,p.x,p.y);pev(t,'pointermove',lp.x,lp.y,0,1);pev(t,'mousemove',lp.x,lp.y,0,1);pev(t,'pointerup',lp.x,lp.y,0,0);pev(t,'mouseup',lp.x,lp.y,0,0,1);if(g.range){setRange(g.el,p,true);out.value=String(g.el.value);}}
var mv=null;if(g.it.isConnected){var np=g.it.parentElement,ks=kidsOf(np),j=ks.indexOf(g.it);if(np!==g.list&&j>=0)mv={text:g.txt,into:placeOf(np),to:j+1,of:ks.length};else if(j>=0&&j!==g.i0)mv={text:g.txt,from:g.i0+1,to:j+1,of:ks.length,before:j+1<ks.length?textOf(ks[j+1]).slice(0,60):'',after:j>0?textOf(ks[j-1]).slice(0,60):''};}
else if(g.list&&g.list.isConnected){var k2=kidsOf(g.list),tx=k2.map(function(k){return textOf(k).slice(0,80);}),j2=tx.indexOf(g.txt);if(j2>=0&&j2!==g.i0)mv={text:g.txt,from:g.i0+1,to:j2+1,of:k2.length,before:j2+1<k2.length?tx[j2+1].slice(0,60):'',after:j2>0?tx[j2-1].slice(0,60):''};}
if(mv)out.moved=mv;out.target=textOf(t).slice(0,80);return out;}
if(op==='hover'){var el=find();if(!el)return {r:'gone'};if(!same(el))return {r:'changed'};if(R.hov&&R.hov!==el&&R.hov.isConnected)leave(R.hov);R.hov=el;into(el);var br=el.getBoundingClientRect(),hx=br.left+br.width/2,hy=br.top+br.height/2;
['pointerover','pointerenter','mouseover','mouseenter','pointermove','mousemove'].forEach(function(k){pev(el,k,hx,hy,0,0);});return {r:'ok'};}
if(op==='check'){var el=find();if(!el)return {r:'gone'};if(!same(el))return {r:'changed'};if(el.disabled)return {r:'disabled'};var isOn=function(){return itype(el)?!!el.checked:el.getAttribute('aria-checked')==='true';};if(isOn()!==!!a.on){into(el);el.click();}return {r:'ok',checked:isOn()};}
if(op==='key'){var t0=a.ref?find():null;if(a.ref){if(!t0)return {r:'gone'};if(!same(t0))return {r:'changed'};into(t0);try{t0.focus({preventScroll:true});}catch(e){}}var kd=[];
for(var ki=0;ki<a.keys.length;ki++){var k=a.keys[ki],cur=focusIn()||D.body;
if(k==='Tab'||k==='Shift+Tab'){var fs=[].slice.call(qa('a[href],button,input:not([type=hidden]),select,textarea,[tabindex],[contenteditable=""],[contenteditable=true]')).filter(function(e){return vis(e)&&!e.disabled&&e.tabIndex>=0;});var j=fs.indexOf(cur);var nx=fs[k==='Tab'?j+1:(j<0?fs.length-1:j-1)];if(nx){into(nx);try{nx.focus({preventScroll:true});}catch(e){}}kd.push(k);continue;}
var kc=({Enter:13,Escape:27,Space:32,ArrowUp:38,ArrowDown:40,ArrowLeft:37,ArrowRight:39,PageUp:33,PageDown:34,Home:36,End:35})[k]||0;
var ko={key:k==='Space'?' ':k,code:k==='Space'?'Space':k,keyCode:kc,which:kc,bubbles:true,cancelable:true,composed:true};
var kev=function(n){var o={};for(var x in ko)o[x]=ko[x];return send(cur,n,o);};
var go=kev('keydown');
if(go){var ct=tag(cur),cr=roleOf(cur);
if(k==='Enter'){if(cur.form&&ct==='input'){if(cur.form.requestSubmit)cur.form.requestSubmit();else cur.form.submit();}else if(ct==='a'||ct==='button'||cr==='button'||cr==='link'){cur.click();}else{kev('keypress');}}
else if(k==='Space'&&(ct==='button'||itype(cur)==='checkbox'||itype(cur)==='radio'||cr==='button')){cur.click();}
else if(ct==='select'&&(k==='ArrowDown'||k==='ArrowUp')){var si=cur.selectedIndex+(k==='ArrowDown'?1:-1);if(si>=0&&si<cur.options.length){cur.selectedIndex=si;fire(cur,'input');fire(cur,'change');}}
else if(bare(cur)||ct==='a'){var sc=scroller(),sh=Math.round((sc?sc.clientHeight:W.innerHeight)*0.85),by=({PageDown:sh,Space:sh,PageUp:-sh,ArrowDown:40,ArrowUp:-40})[k];if(by)sBy(sc,by);else if(k==='Home')sTo(sc,0);else if(k==='End')sTo(sc,sc?sc.scrollHeight:D.documentElement.scrollHeight);}}
kev('keyup');
if(k==='Escape'&&R.hov){if(R.hov.isConnected)leave(R.hov);R.hov=null;}
kd.push(k);}
var fe=focusIn();return {r:'ok',keys:kd,focused:bare(fe)?null:desc(fe),scroll:scrollAt()};}
if(op==='snap'){var main=mainEl();var all=qa(SEL+',iframe,frame'),els=[],more=false;var wi=null,whl=0,wst=null,hlv=function(e){var m=/^h([1-6])$/.exec(tag(e));if(m)return +m[1];if((e.getAttribute('role')||'').toLowerCase()==='heading')return +(e.getAttribute('aria-level')||2)||2;return 0;};if(a.within){wi=R.els[a.within];if(!wi||!wi.isConnected)return {title:sq(D.title),url:location.href,text:'',headings:[],elements:[],forms:[],password:false,payment:false,more:false,gone:true};whl=hlv(wi);if(whl){var hs2=wi.ownerDocument.querySelectorAll('h1,h2,h3,h4,h5,h6,[role=heading]');for(var j=0;j<hs2.length;j++){var h2=hs2[j];if(h2===wi||wi.contains(h2)||h2.contains(wi)||!(wi.compareDocumentPosition(h2)&4))continue;if(hlv(h2)<=whl){wst=h2;break;}}}}var inside=function(x){if(!wi)return true;if(!whl)return wi===x||wi.contains(x);var p=wi.compareDocumentPosition(x);if(!(x===wi||(p&4)||(p&16)))return false;return !wst||!!(wst.compareDocumentPosition(x)&2);};
for(var i=0;i<all.length;i++){var ae2=all[i];if(!vis(ae2)||!inside(ae2))continue;if(ae2.tagName==='IFRAME'||ae2.tagName==='FRAME'){if(fdoc(ae2))continue;var fb=ae2.getBoundingClientRect();if(fb.width<20||fb.height<20)continue;}if(els.length>=a.max){more=true;break;}els.push(desc(ae2));}
return {title:sq(D.title),url:location.href,text:main&&a.text?sq(shownText(main)).slice(0,a.text):'',headings:heads(),elements:els,forms:formsInfo(),password:!!q1('input[type=password]'),payment:takesCard(D),more:more,scroll:scrollAt()};}
if(op==='describe')return base(find());
if(op==='tree'){var IMP={a:'link',button:'button',select:'combobox',textarea:'textbox',img:'img',svg:'img',ul:'list',ol:'list',li:'listitem',nav:'navigation',main:'main',header:'banner',footer:'contentinfo',aside:'complementary',form:'form',table:'table',tr:'row',th:'columnheader',td:'cell',p:'paragraph',dialog:'dialog',details:'group',summary:'button',section:'region',article:'article',label:'label',fieldset:'group',option:'option',video:'video',audio:'audio',iframe:'iframe',h1:'heading',h2:'heading',h3:'heading',h4:'heading',h5:'heading',h6:'heading',hr:'separator',blockquote:'blockquote',figure:'figure',figcaption:'caption',pre:'code'};var INTER={link:1,button:1,textbox:1,searchbox:1,combobox:1,checkbox:1,radio:1,slider:1,spinbutton:1,'switch':1,tab:1,menuitem:1,menuitemcheckbox:1,menuitemradio:1,option:1,treeitem:1};var KEEP={heading:1,list:1,listitem:1,navigation:1,main:1,banner:1,contentinfo:1,complementary:1,form:1,table:1,row:1,cell:1,columnheader:1,dialog:1,alertdialog:1,region:1,article:1,img:1,paragraph:1,group:1,iframe:1,figure:1,separator:1,tablist:1,tabpanel:1,menu:1,menubar:1,listbox:1,alert:1,status:1,blockquote:1,code:1,caption:1,label:1,video:1,audio:1};var NAMED={heading:1,listitem:1,cell:1,columnheader:1,paragraph:1,caption:1,label:1,option:1,blockquote:1};var SKIP={script:1,style:1,noscript:1,template:1,meta:1,link:1,head:1,title:1};var trole=function(e){var r=(e.getAttribute('role')||'').toLowerCase().split(' ')[0];if(r&&r!=='presentation'&&r!=='none')return r;var t=tag(e);if(t==='input'){var ty=itype(e);return ty==='hidden'?'':roleOf(e);}if(t==='a'&&!e.hasAttribute('href'))return '';return IMP[t]||'';};var tlv=function(e){var m=/^h([1-6])$/.exec(tag(e));return m?+m[1]:+(e.getAttribute('aria-level')||0);};var town=function(e){var s='';for(var c=e.firstChild;c;c=c.nextSibling){if(c.nodeType===3)s+=c.nodeValue;}return sq(s);};var tname=function(e,r){var l=e.getAttribute('aria-label')||e.getAttribute('alt')||(r==='img'?e.getAttribute('title'):'')||'';if(l)return sq(l);var lb=e.getAttribute('aria-labelledby');if(lb){var lt=lb.split(/\s+/).map(function(i){var x=D.getElementById(i);return x?x.textContent:'';}).join(' ');if(sq(lt))return sq(lt);}if(r==='table'&&e.caption)return sq(e.caption.textContent);if(INTER[r]){var nn=nameOf(e);if(nn)return sq(nn);}if(INTER[r]||NAMED[r])return sq(e.innerText||e.textContent||'');if(r==='img'&&tag(e)==='svg'){var tt=e.querySelector('title');return tt?sq(tt.textContent):'';}return '';};var tvis=function(e){if(SKIP[tag(e)])return false;if(tag(e)==='input'&&itype(e)==='hidden')return false;if(e.getAttribute('aria-hidden')==='true')return false;if(cs(e).display==='contents')return true;if(e.checkVisibility)return e.checkVisibility({visibilityProperty:true});return vis(e);};var wantR=String(a.role||'').toLowerCase(),wantW=String(a.words||'').toLowerCase(),lim=Math.max(1,Number(a.limit)||1e9),only=a.filter==='interactive',maxD=Math.max(1,Math.min(40,Number(a.depth)||15)),from=Math.max(0,Number(a.offset)||0),max=Math.max(500,Number(a.max)||5000),cap=from+max+1,out=[],len=0;var add=function(s){if(len>=cap)return;out.push(s);len+=s.length+1;};var walk=function(e,d,q){if(len>=cap||(wantR&&out.length>=lim))return;var kids=[].slice.call(e.shadowRoot?e.shadowRoot.children:[]).concat([].slice.call(e.children));for(var i=0;i<kids.length&&len<cap;i++){var c=kids[i];if(!tvis(c))continue;var r=trole(c),inter=!!INTER[r]||(!r&&c.hasAttribute('tabindex')&&c.tabIndex>=0)||(!r&&typeof c.onclick==='function'),n=tname(c,r).slice(0,100),tx=(!n&&!INTER[r])?town(c).slice(0,100):'';if(wantR){if(r===wantR&&(!wantW||(n+' '+tx+' '+(n?'':sq(c.innerText||'').slice(0,400))).toLowerCase().indexOf(wantW)>=0)){var sd=n||tx||sq(c.innerText||'').slice(0,100);add('['+refOf(c)+'] '+r+(sd?' "'+sd.replace(/"/g,"'")+'"':''));}walk(c,d+1,q);continue;}var show=only?inter:(inter||!!KEEP[r]||(!q&&(!!n||!!tx)));if(show){var line=new Array(d+1).join('  ')+'- '+(r||(inter?'clickable':tx?'text':'generic'));if(r==='heading'){var lv=tlv(c);if(lv)line+=' (level '+lv+')';}var said=n||tx;if(said)line+=' "'+said.replace(/"/g,"'")+'"';if(tag(c)==='a'&&c.href&&/^https?:/i.test(c.href))line+=' → '+String(c.href).slice(0,120);if(c.disabled)line+=' (disabled)';if(c.checked)line+=' (checked)';var ex=c.getAttribute('aria-expanded');if(ex)line+=' ('+(ex==='true'?'expanded':'collapsed')+')';if(c.getAttribute('aria-selected')==='true')line+=' (selected)';line+=' ['+refOf(c)+']';add(line);if(d+1<maxD&&!(INTER[r]&&r!=='option'&&n))walk(c,d+1,q||!!(n&&(NAMED[r]||INTER[r])));}else walk(c,d,q);}};var troot=D.body||D.documentElement;if(a.within){troot=R.els[a.within];if(!troot||!troot.isConnected)return {r:'gone'};}walk(troot,0,false);var all=out.join('\n'),end=Math.min(all.length,from+max);if(end<all.length){var nl=all.lastIndexOf('\n',end);if(nl>from)end=nl+1;}return {r:'ok',text:all.slice(from,end),more:end<all.length,end:end,lines:out.length};}
if(op==='wheelAim'){var wx=Number(a.x),wy=Number(a.y),wt=ptAt(wx,wy),ch=[];for(var we=wt;we&&ch.length<12;we=we.parentElement||(we.getRootNode&&we.getRootNode().host)||null){if(we.scrollWidth>we.clientWidth+2||we.scrollHeight>we.clientHeight+2)ch.push({e:we,l:we.scrollLeft,t:we.scrollTop});}R.wh={ch:ch,x:W.scrollX,y:W.scrollY};var wb=ch.length?ch[0].e:null;return {r:'ok',parts:ch.length,w:Math.round(wb&&wb.clientWidth>0?Math.min(wb.clientWidth,W.innerWidth):W.innerWidth),h:Math.round(wb&&wb.clientHeight>0?Math.min(wb.clientHeight,W.innerHeight):W.innerHeight)};}
if(op==='wheeled'){var wh=R.wh;if(!wh)return {r:'ok',moved:false};var se=D.scrollingElement||D.documentElement,mv=null;for(var wi=0;wi<wh.ch.length;wi++){var c0=wh.ch[wi];if(c0.e.isConnected&&(c0.e.scrollLeft!==c0.l||c0.e.scrollTop!==c0.t)){mv=c0.e;break;}}var pg=W.scrollX!==wh.x||W.scrollY!==wh.y;if(!mv&&!pg)return {r:'ok',moved:false};if(!mv||mv===se||mv===D.body)return {r:'ok',moved:true,part:'the page'};var wr=(mv.getAttribute('role')||'').toLowerCase(),wn=sq(mv.getAttribute('aria-label')||''),what=mv.querySelector('table')||tag(mv)==='table'?'a table':mv.querySelector('pre,code')||tag(mv)==='pre'?'a code block':wr?'a '+wr:'a part that scrolls by itself';return {r:'ok',moved:true,part:what+(wn?' (‘'+wn.slice(0,60)+'’)':''),ref:refOf(mv),pos:{x:Math.round(mv.scrollLeft),max_x:Math.max(0,Math.round(mv.scrollWidth-mv.clientWidth)),y:Math.round(mv.scrollTop),max:Math.max(0,Math.round(mv.scrollHeight-mv.clientHeight))}};}
if(op==='aim'){var ae=find();if(!ae)return {r:'gone'};if(!same(ae))return {r:'changed'};if(ae.disabled)return {r:'disabled'};var atg=tag(ae)==='a'?(ae.getAttribute('target')||'').toLowerCase():'';if(atg&&atg!=='_self'&&atg!=='_parent'&&atg!=='_top'&&/^https?:/i.test(ae.href))return {r:'script'};var ax,ay,ah;if(a.at){ax=Number(a.x);ay=Number(a.y);ah=ptAt(ax,ay);if(!ah||!holds(ae,ah))return {r:'changed'};}else{if(!shown(ae))into(ae);var ac=rr(ae);ax=ac.left+ac.width/2;ay=ac.top+ac.height/2;ah=ptAt(ax,ay);if(ah&&!holds(ae,ah)){into(ae);ac=rr(ae);ax=ac.left+ac.width/2;ay=ac.top+ac.height/2;ah=ptAt(ax,ay);}}var acv='';if(ah&&!holds(ae,ah)){var aco=ah.closest('dialog,[role=dialog],[role=alertdialog],[aria-modal=true]')||ah;acv=sq(aco.getAttribute('aria-label')||aco.innerText||aco.textContent||'').slice(0,60);var acb=[].slice.call(aco.querySelectorAll('button,[role=button],a[href]')).filter(function(b){return shown(b)&&sq(b.innerText||b.getAttribute('aria-label')||'');}).slice(0,4).map(function(b){return '['+refOf(b)+'] '+sq(b.innerText||b.getAttribute('aria-label')||'').slice(0,30);});if(acb.length)acv+=' — its buttons: '+acb.join(', ');}var aks=a.kinds||['mousedown','click'],aws=[window];if(win(ae)!==window)aws.push(win(ae));var ast={id:Math.random().toString(36).slice(2),n:0,ws:aws,ks:aks,h:null};ast.h=function(e){if(e.isTrusted&&(!a.on||holds(ae,e.target)))ast.n++;};aws.forEach(function(w){aks.forEach(function(k){w.addEventListener(k,ast.h,true);});});if(window.__yapAim){try{var old=window.__yapAim;old.ws.forEach(function(w){old.ks.forEach(function(k){w.removeEventListener(k,old.h,true);});});}catch(e){}}window.__yapAim=ast;return {r:'ok',id:ast.id,x:ax,y:ay,covered:acv};}
if(op==='listen'){var le=null;if(a.ref){le=find();if(!le)return {r:'gone'};if(!same(le))return {r:'changed'};try{le.focus({preventScroll:false});}catch(e){}}var lks=a.kinds||['keydown'],lws=[window],lf=le||document.activeElement;if(lf&&win(lf)!==window)lws.push(win(lf));var lst={id:Math.random().toString(36).slice(2),n:0,ws:lws,ks:lks,h:null};lst.h=function(e){if(e.isTrusted)lst.n++;};if(window.__yapAim){try{var lo=window.__yapAim;lo.ws.forEach(function(w){lo.ks.forEach(function(k){w.removeEventListener(k,lo.h,true);});});}catch(e){}}lws.forEach(function(w){lks.forEach(function(k){w.addEventListener(k,lst.h,true);});});window.__yapAim=lst;return {r:'ok',id:lst.id};}
if(op==='unhover'){if(R.hov&&R.hov.isConnected)leave(R.hov);R.hov=null;return {r:'ok'};}
if(op==='aimed'){var ad=window.__yapAim;if(!ad||ad.id!==a.id)return {r:'ok',heard:-1};ad.ws.forEach(function(w){ad.ks.forEach(function(k){try{w.removeEventListener(k,ad.h,true);}catch(e){}});});window.__yapAim=null;var ao={r:'ok',heard:ad.n};if(a.hover&&ad.n){var hv=find();if(hv)R.hov=hv;}if(Number(a.count)>1){var as=find();if(as)ao.selected=selNow(as);}return ao;}
if(op==='where'){var wh=find();if(!wh)return {r:'gone'};return {r:'ok',scroll:scrollAt(),inView:inView(),seen:shown(wh)};}
if(op==='section'){var se=find();if(!se)return {r:'gone'};var hl=function(e){var t=tag(e),m=/^h([1-6])$/.exec(t);if(m)return +m[1];if((e.getAttribute('role')||'').toLowerCase()==='heading')return +(e.getAttribute('aria-level')||2)||2;return 0;};
var lv=hl(se),dc=se.ownerDocument,stop=null;if(lv){var hs=dc.querySelectorAll('h1,h2,h3,h4,h5,h6,[role=heading]');for(var i=0;i<hs.length;i++){var h=hs[i];if(h===se||se.contains(h)||h.contains(se)||!(se.compareDocumentPosition(h)&4))continue;if(hl(h)<=lv){stop=h;break;}}}
var from=Math.max(0,Number(a.offset)||0),cap=Math.max(1,Number(a.max)||5000),want=from+cap+1,root=lv?(dc.body||dc.documentElement):se,w=dc.createTreeWalker(root,4),bl=new Map(),parts=[],last=null,len=0,tx;
var blockOf=function(e){for(var x=e;x&&x!==root.parentNode;x=x.parentElement){if(bl.has(x))return bl.get(x);var d=(win(x).getComputedStyle(x).display||'');if(d.indexOf('inline')!==0&&d!=='contents'){bl.set(x,x);return x;}}return root;};
var skip=function(e){if(!e||e.closest('script,style,noscript,template'))return true;return e.checkVisibility?!e.checkVisibility({visibilityProperty:true}):false;};
if(lv)w.currentNode=se;
while((tx=w.nextNode())&&len<want){if(stop&&(stop===tx||stop.contains(tx)||(stop.compareDocumentPosition(tx)&4)))break;var p=tx.parentElement,s=(tx.nodeValue||'').replace(/\s+/g,' ');if(!s.trim()||skip(p))continue;var b=blockOf(p);if(b!==last&&parts.length){parts.push('\n');}else if(parts.length&&!/\s$/.test(parts[parts.length-1])&&!/^\s/.test(s)){parts.push(' ');}parts.push(s);last=b;len+=s.length;}
var all=parts.join('').replace(/[ \t]*\n[ \t]*/g,'\n').replace(/\n{2,}/g,'\n').replace(/^\s+|\s+$/g,'');
return {r:'ok',heading:lv?sq(se.innerText||se.textContent).slice(0,160):'',level:lv,text:all.slice(from,from+cap),more:all.length>from+cap};}
if(op==='scroll'){if(a.ref){var s=find();if(!s)return {r:'gone'};into(s);return {r:'ok',scroll:scrollAt(),inView:inView(),seen:shown(s)};}
var sc=scroller(),b0=scrollAt(),d=a.to||'down',n=Math.max(0.1,Math.min(10,Number(a.amount)||1)),step=Math.round((sc?sc.clientHeight:W.innerHeight)*0.85*n);
if(d==='left'||d==='right'){var hs=Math.round(W.innerWidth*0.85*n)*(d==='left'?-1:1);sBy(sc&&sc.scrollWidth>sc.clientWidth+2?sc:null,0,hs);}else if(d==='top')sTo(sc,0);else if(d==='bottom')sTo(sc,sc?sc.scrollHeight:D.documentElement.scrollHeight);else sBy(sc,(d==='up'?-1:1)*step);
var b1=scrollAt();return {r:'ok',scroll:b1,moved:b1.x!==b0.x||b1.y!==b0.y,inView:inView()};}
var el=find();if(!el)return {r:'gone'};if(!same(el))return {r:'changed'};if(el.disabled)return {r:'disabled'};
var caretIn=(function(){var s=selIn(el);return !!(s&&s.rangeCount&&el.contains(s.anchorNode));})();
into(el);try{el.focus({preventScroll:true});}catch(e){}
if(op==='click'){var tg=tag(el)==='a'?(el.getAttribute('target')||'').toLowerCase():'';
if(tg&&tg!=='_self'&&tg!=='_parent'&&tg!=='_top'&&/^https?:/i.test(el.href)){location.assign(el.href);return {r:'ok',went:true};}
if(!shown(el))into(el);var c1=rr(el),qx=c1.left+c1.width/2,qy=c1.top+c1.height/2,hq=ptAt(qx,qy),cv='';if(hq&&!holds(el,hq)){var co=hq.closest('dialog,[role=dialog],[role=alertdialog],[aria-modal=true]')||hq;cv=sq(co.getAttribute('aria-label')||co.innerText||co.textContent||'').slice(0,60);var cb=[].slice.call(co.querySelectorAll('button,[role=button],a[href]')).filter(function(b){return shown(b)&&sq(b.innerText||b.getAttribute('aria-label')||'');}).slice(0,4).map(function(b){return '['+refOf(b)+'] '+sq(b.innerText||b.getAttribute('aria-label')||'').slice(0,30);});if(cb.length)cv+=' — its buttons: '+cb.join(', ');}if(!hq||!holds(el,hq))hq=el;var q=loc(hq,qx,qy);
pev(hq,'pointerdown',q.x,q.y,0,1);pev(hq,'mousedown',q.x,q.y,0,1,1);try{el.focus({preventScroll:true});}catch(e){}pev(hq,'pointerup',q.x,q.y,0,0);pev(hq,'mouseup',q.x,q.y,0,0,1);pev(hq,'click',q.x,q.y,0,0,1);return cv?{r:'ok',covered:cv}:{r:'ok'};}
if(op==='choose'){var want=String(a.text).replace(/\s+/g,' ').trim().toLowerCase(),ct2=tag(el),cty=itype(el);
var pk=function(list,txt){var i;for(i=0;i<list.length;i++){if(txt(list[i]).toLowerCase()===want)return list[i];}for(i=0;i<list.length;i++){if(txt(list[i]).toLowerCase().indexOf(want)>=0)return list[i];}return null;};
if(ct2==='select'){var op2=[].slice.call(el.options),po2=pk(op2,function(x){return sq(x.text);})||pk(op2,function(x){return String(x.value);});if(!po2)return {r:'no-option'};el.value=po2.value;fire(el,'input');fire(el,'change');return {r:'ok',value:sq(po2.text)};}
if(cty==='radio'){var rh=pk(group(el),function(x){return labelOf(x)||sq(x.value);});if(!rh)return {r:'no-option'};if(rh.disabled)return {r:'disabled'};into(rh);if(!rh.checked)rh.click();return {r:'ok',value:labelOf(rh)||sq(rh.value),checked:!!rh.checked};}
var shut=function(){var ek=function(){return {key:'Escape',code:'Escape',keyCode:27,which:27,bubbles:true,cancelable:true,composed:true};};send(el,'keydown',ek());send(el,'keyup',ek());};
var oe=pk(optEls(el),function(x){return sq(x.textContent);});
if((!oe||!vis(oe))&&!a.again){el.click();oe=pk(optEls(el),function(x){return sq(x.textContent);});if((!oe&&!optEls(el).length)||(oe&&!vis(oe)))return {r:'opening'};}
if(!oe){var some=optsOf(el);shut();return {r:'no-option',options:some};}
into(oe);pev(oe,'pointerdown',0,0,0,1);pev(oe,'mousedown',0,0,0,1,1);pev(oe,'pointerup',0,0,0,0);pev(oe,'mouseup',0,0,0,0,1);oe.click();
var lb=oe.closest('[role=listbox]');if(lb&&shown(lb))shut();
var so2=selOpt(el);return {r:'ok',value:so2?sq(so2.textContent):sq(oe.textContent)};}
if(op==='type'){var t=tag(el);
if(t==='select'){var want=String(a.text).toLowerCase().trim(),pick=null;for(var i=0;i<el.options.length;i++){var o=el.options[i];if(sq(o.text).toLowerCase()===want||String(o.value).toLowerCase()===want){pick=o;break;}}
if(!pick){for(var j=0;j<el.options.length;j++){if(sq(el.options[j].text).toLowerCase().indexOf(want)>=0){pick=el.options[j];break;}}}if(!pick)return {r:'no-option'};el.value=pick.value;fire(el,'input');fire(el,'change');}
else if(t==='input'||t==='textarea'){var ew=win(el),proto=t==='textarea'?ew.HTMLTextAreaElement.prototype:ew.HTMLInputElement.prototype;var set=Object.getOwnPropertyDescriptor(proto,'value').set;set.call(el,String(a.text));fire(el,'input');fire(el,'change');}
else if(el.isContentEditable){var sel=selIn(el),txt=String(a.text),od=el.ownerDocument;
if(txt){if(!caretIn||!sel.rangeCount){var rg=od.createRange();rg.selectNodeContents(el);rg.collapse(false);sel.removeAllRanges();sel.addRange(rg);var had=el.innerText||'';if(had.trim()&&!/\s$/.test(had))txt=' '+txt;}
var done=false;try{done=od.execCommand('insertText',false,txt);}catch(e){}
if(!done){var r2=sel.getRangeAt(0);r2.insertNode(od.createTextNode(txt));r2.collapse(false);fire(el,'input');}}}
else return {r:'not-a-field'};
var ins=el.isContentEditable&&t!=='input'&&t!=='textarea';
var now=secret(el)?'':(t==='select'?sq(el.options[el.selectedIndex]?el.options[el.selectedIndex].text:''):(ins?sq(el.innerText):String(el.value||'')).slice(0,200));
if(a.submit){if(el.form){if(el.form.requestSubmit)el.form.requestSubmit();else el.form.submit();}else{['keydown','keypress','keyup'].forEach(function(k){send(el,k,{key:'Enter',code:'Enter',keyCode:13,which:13,bubbles:true,composed:true});});}}
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
/// most `max` elements — only those inside element `within` (`browser_read { mode: "elements",
/// ref }`): a heading's section, up to the next heading at its level or
/// above, or the element itself; `gone` when it isn't there now.
pub fn snap_within(text: usize, max: usize, within: Option<u32>) -> String {
    build("snap", serde_json::json!({ "text": text, "max": max, "within": within }))
}

/// Describe element `r` as it is now.
pub fn describe(r: u32) -> String {
    build("describe", serde_json::json!({ "ref": r }))
}

/// Click element `r`, if it's still `role` "`name`" — **as a person's mouse
/// does** (14 Nov 2026; before, the element's own `click()`, which a page's
/// toggle laid over a heading never heard, and nor did a menu that opens on
/// `mousedown`): scrolled into view, the element on top at its centre (one
/// inside it: a heading's invisible toggle, a button's icon; else the
/// element itself) gets pointerdown, mousedown, pointerup, mouseup and click
/// there, as [`press`] sends them.
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

/// **Just one part of the page** (`browser_read { ref }`): element `r`'s
/// section when it's a heading — the heading and what follows it, up to the
/// next heading at its level or above — else its own words; what the
/// person sees of it (nothing hidden, no script or style), a line a block,
/// from character `offset`, `max` of them, and whether there's `more`.
pub fn section(r: u32, offset: usize, max: usize) -> String {
    build("section", serde_json::json!({ "ref": r, "offset": offset, "max": max }))
}

/// **The page's structure** (`browser_read { mode: "tree" }`, as Claude Code's
/// own browser's `read_page`): an accessibility tree, a line a node — its
/// role (a heading's level), its name or words, a link's address, its state
/// (checked, expanded, selected, disabled) and its ref — indented by depth
/// (at most `depth`), generic boxes with nothing to say folded into their
/// parent; `filter` "interactive": the links, buttons and fields alone;
/// `within`: one element's inside. From character `offset`, `max` of it.
pub fn tree(within: Option<u32>, depth: u64, filter: &str, offset: usize, max: usize) -> String {
    build("tree", serde_json::json!({ "within": within, "depth": depth, "filter": filter, "offset": offset, "max": max }))
}

/// **The nodes of one role** (`browser_find "table"`, as Claude Code's own
/// browser's find matches any role): \`[ref] role "words"\` a line, those
/// holding `words` (any, when empty), at most `limit`.
pub fn roles(role: &str, words: &str, limit: usize) -> String {
    build("tree", serde_json::json!({ "role": role, "words": words, "limit": limit, "depth": 40, "max": 20_000 }))
}

/// **Before the wheel at (`x`, `y`)**: what's under it that scrolls, and
/// where it and the page are, for [`wheeled`].
pub fn wheel_aim(x: f64, y: f64) -> String {
    build("wheelAim", serde_json::json!({ "x": x, "y": y }))
}

/// **After the wheel**: whether anything moved, and what — `part` ("the
/// page", "a table", "a code block", "a list"…) and its ref.
pub fn wheeled() -> String {
    build("wheeled", serde_json::json!({}))
}

/// **Before real input on element `r`** (if it's still `role` "`name`"):
/// the point to press — its centre, scrolled into view, or (`at`) the
/// shot's point, still on it — what's on top of it there (a dialog's words
/// and its buttons by ref), and a listener for the real events `kinds`
/// ("mousedown", "click"…) in its page and frame. `{r: "script"}`: a
/// link to another window, which the script's own click takes.
pub fn aim(r: u32, role: &str, name: &str, at: Option<(f64, f64)>, kinds: &[&str], on: bool) -> String {
    let (x, y) = at.unwrap_or((0.0, 0.0));
    build("aim", serde_json::json!({ "ref": r, "role": role, "name": name, "at": at.is_some(), "x": x, "y": y, "kinds": kinds, "on": on }))
}

/// **Before real keys**: element `r` focused (if it's still `role`
/// "`name`"; `None`: the focus as it is) and a listener for the real
/// events `kinds` ("keydown"); `aimed` reads it.
pub fn listen(target: Option<(u32, &str, &str)>, kinds: &[&str]) -> String {
    let (r, role, name) = target.unwrap_or((0, "", ""));
    build("listen", serde_json::json!({ "ref": r, "role": role, "name": name, "kinds": kinds }))
}

/// **After real input** aimed at element `r` (`id`: `aim`'s): `heard`, how
/// many of the real events the page got (-1: the page went — a link
/// followed), and the words a double or triple click (`count`) selected.
pub fn aimed(r: u32, id: &str, count: u8, hover: bool) -> String {
    build("aimed", serde_json::json!({ "ref": r, "id": id, "count": count, "hover": hover }))
}

/// **The hovered element let go** (after a real Escape): the pointer leaves
/// it, as the script's own Escape does, so a hover's popup closes.
pub fn unhover() -> String {
    build("unhover", serde_json::json!({}))
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

/// **A press of another button, or more than one click** on element `r`
/// (if it's still `role` "`name`"): at (`x`, `y`) of the view (`at`, and
/// still that element there), else at its middle (scrolled into view) —
/// `button` 0 (left, `count` 1–3: a double or triple click, the words there
/// selected as a person's would be unless the page holds the mouse down
/// itself), 1 (middle: `auxclick`), 2 (right: `contextmenu`, `menu` true when
/// the page took it for a menu of its own). Its answer: `selected` (the
/// words selected after a double or triple click), `menu`.
pub fn press(r: u32, role: &str, name: &str, at: Option<(f64, f64)>, button: u8, count: u8) -> String {
    let (x, y) = at.unwrap_or((0.0, 0.0));
    build("press", serde_json::json!({ "ref": r, "role": role, "name": name, "at": at.is_some(), "x": x, "y": y, "button": button, "count": count }))
}

/// **A drag begun** from element `r` (if it's still `role` "`name`"), at (`x`,
/// `y`) of the view (`at`) or its middle, toward `to` (`{ref, role, name}`, or
/// `{at: true, x, y}`): the pointer down there and, when what's under it is
/// draggable (HTML5 drag and drop: `dragstart` not cancelled), a `DataTransfer`
/// of its own. Then [`drag_move`] a step at a time, and [`drag_end`].
pub fn drag_start(r: u32, role: &str, name: &str, at: Option<(f64, f64)>, to: serde_json::Value) -> String {
    let (x, y) = at.unwrap_or((0.0, 0.0));
    build("dragStart", serde_json::json!({ "ref": r, "role": role, "name": name, "at": at.is_some(), "x": x, "y": y, "to": to }))
}

/// The drag `f` (0–1) of the way to where it ends: pointer and mouse moves
/// over what's under it (`dragover` for an HTML5 drag; a slider's value
/// follows).
pub fn drag_move(f: f64) -> String {
    build("dragMove", serde_json::json!({ "f": f }))
}

/// The drag ended where it was going: the drop (an HTML5 drag the target
/// takes: `dropped`), or the pointer up. Its answer: `moved` (the list item
/// it dragged, where it is now), `value` (a slider's), `target` (the words
/// where it ended).
pub fn drag_end() -> String {
    build("dragEnd", serde_json::json!({}))
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
        assert!(snap_within(2500, 150, None).contains("})(\"snap\",{"));
        assert!(snap_within(0, 150, Some(9)).contains("\"within\":9"));
        assert!(aim(3, "heading", "Order", None, &["mousedown"], false).contains("})(\"aim\",{"));
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
