//! **Yap's own scripts for acting on a page** (run in the page's own
//! webview through `view::run_script`; never a script a page or a model
//! wrote — the model's words only ever go in as a JSON string).
//!
//! One small library ([`LIB`]) and an operation:
//! - `snap`: the page's title, address, main text, headings, and its links,
//!   buttons and fields (visible ones, in the page's order, at most
//!   `rules::MAX_ELEMENTS`), each with its ref; its forms (a password field?
//!   where it goes, its submit button's words); where it's scrolled to.
//! - `describe`: one element by its ref, as it is now, with its form.
//! - `click`, `type`, `scroll`: the action — only if the element is still
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
function secret(el){var ty=itype(el),ac=(el.getAttribute('autocomplete')||'').toLowerCase();if(ty==='password'||/(^|\s)(cc-|current-password|new-password|one-time-code)/.test(ac))return true;
var s=[el.getAttribute('name'),el.id,el.getAttribute('placeholder'),el.getAttribute('aria-label'),labelOf(el)].join(' ');return /card|cvv|cvc|csc|iban|sort.?code|account.?n(o|um)|security.?code|pass(word|code)|\bpin\b|otp/i.test(s);}
function submitBtn(f){return f?f.querySelector('button:not([type=button]):not([type=reset]),input[type=submit],input[type=image]'):null;}
function formsInfo(){return F.map(function(f,i){var b=submitBtn(f);return {form:i,password:!!f.querySelector('input[type=password]'),action:f.action||'',method:(f.getAttribute('method')||'get').toLowerCase(),submit:b?nameOf(b):''};});}
function desc(el){var t=tag(el),ty=itype(el),m=secret(el);
var d={ref:refOf(el),role:roleOf(el),name:nameOf(el).slice(0,120),type:ty,autocomplete:el.getAttribute('autocomplete')||'',field:el.getAttribute('name')||'',id:el.id||'',label:labelOf(el),placeholder:el.getAttribute('placeholder')||'',href:t==='a'?(el.href||''):'',disabled:!!el.disabled||el.getAttribute('aria-disabled')==='true',form:el.form?F.indexOf(el.form):-1,submit:(t==='button'&&(el.getAttribute('type')||'submit').toLowerCase()==='submit'&&!!el.form)||(t==='input'&&(ty==='submit'||ty==='image')),masked:m,value:'',options:[]};
if(ty==='checkbox'||ty==='radio'||d.role==='checkbox'||d.role==='radio'||d.role==='switch'){d.checked=ty?!!el.checked:el.getAttribute('aria-checked')==='true';}
else if(t==='select'){var o=el.options[el.selectedIndex];d.value=o?sq(o.text):'';for(var i=0;i<el.options.length&&i<20;i++)d.options.push(sq(el.options[i].text));}
else if(!m&&(t==='input'||t==='textarea')&&['submit','button','reset','image'].indexOf(ty)<0){d.value=String(el.value||'').slice(0,200);}
else if(!m&&el.isContentEditable){d.value=sq(el.innerText).slice(0,200);}
return d;}
var SEL='a[href],button,input:not([type=hidden]),select,textarea,summary,[role=button],[role=link],[role=checkbox],[role=radio],[role=tab],[role=menuitem],[role=switch],[role=combobox],[role=textbox],[role=searchbox],[role=option],[contenteditable=""],[contenteditable=true]';
function scrollAt(){var de=D.documentElement;return {y:Math.round(W.scrollY),h:Math.round(W.innerHeight),max:Math.max(0,Math.round((de?de.scrollHeight:0)-W.innerHeight))};}
function heads(){return [].slice.call(D.querySelectorAll('h1,h2,h3')).slice(0,30).map(function(h){return sq(h.innerText);}).filter(Boolean);}
function find(){var el=R.els[a.ref];return el&&el.isConnected?el:null;}
function same(el){var d=desc(el);return d.role===a.role&&d.name===a.name;}
function fire(el,k){el.dispatchEvent(new Event(k,{bubbles:true}));}
if(op==='snap'){var main=D.querySelector('main,article,[role=main]')||D.body;var all=D.querySelectorAll(SEL),els=[],more=false;
for(var i=0;i<all.length;i++){if(!vis(all[i]))continue;if(els.length>=a.max){more=true;break;}els.push(desc(all[i]));}
return {title:sq(D.title),url:location.href,text:main?sq(main.innerText).slice(0,a.text):'',headings:heads(),elements:els,forms:formsInfo(),password:!!D.querySelector('input[type=password]'),more:more,scroll:scrollAt()};}
if(op==='describe'){var el=find();var base={password:!!D.querySelector('input[type=password]'),title:sq(D.title),headings:heads(),url:location.href};
if(!el){base.el=null;return base;}base.el=desc(el);base.form=el.form?formsInfo()[F.indexOf(el.form)]:null;return base;}
if(op==='scroll'){if(a.ref){var s=find();if(!s)return {r:'gone'};s.scrollIntoView({block:'center'});}else{W.scrollBy(0,(a.to==='up'?-1:1)*Math.round(W.innerHeight*0.8));}return {r:'ok',scroll:scrollAt()};}
var el=find();if(!el)return {r:'gone'};if(!same(el))return {r:'changed'};if(el.disabled)return {r:'disabled'};
try{el.scrollIntoView({block:'center'});}catch(e){}try{el.focus();}catch(e){}
if(op==='click'){el.click();return {r:'ok'};}
if(op==='type'){var t=tag(el);
if(t==='select'){var want=String(a.text).toLowerCase().trim(),pick=null;for(var i=0;i<el.options.length;i++){var o=el.options[i];if(sq(o.text).toLowerCase()===want||String(o.value).toLowerCase()===want){pick=o;break;}}
if(!pick){for(var j=0;j<el.options.length;j++){if(sq(el.options[j].text).toLowerCase().indexOf(want)>=0){pick=el.options[j];break;}}}if(!pick)return {r:'no-option'};el.value=pick.value;fire(el,'input');fire(el,'change');}
else if(t==='input'||t==='textarea'){var proto=t==='textarea'?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;var set=Object.getOwnPropertyDescriptor(proto,'value').set;set.call(el,String(a.text));fire(el,'input');fire(el,'change');}
else if(el.isContentEditable){el.textContent=String(a.text);fire(el,'input');}
else return {r:'not-a-field'};
var now=secret(el)?'':(t==='select'?sq(el.options[el.selectedIndex]?el.options[el.selectedIndex].text:''):String(el.value||el.innerText||'').slice(0,200));
if(a.submit){if(el.form){if(el.form.requestSubmit)el.form.requestSubmit();else el.form.submit();}else{['keydown','keypress','keyup'].forEach(function(k){el.dispatchEvent(new KeyboardEvent(k,{key:'Enter',code:'Enter',keyCode:13,which:13,bubbles:true}));});}}
return {r:'ok',value:now};}
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

/// Scroll the page up or down a screen, or element `r` into view.
pub fn scroll(to_up: bool, r: Option<u32>) -> String {
    build("scroll", serde_json::json!({ "to": if to_up { "up" } else { "down" }, "ref": r }))
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
        assert!(scroll(true, None).contains("\"to\":\"up\""));
        // Never a secret's value: the library masks them by type, autocomplete and name.
        assert!(LIB.contains("current-password|new-password|one-time-code") && LIB.contains("masked:m"));
        // Every action checks the element is still the one described first.
        assert!(LIB.contains("if(!same(el))return {r:'changed'}"));
    }
}
