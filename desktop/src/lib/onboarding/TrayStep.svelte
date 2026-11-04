<script>
  // Setup's tray step: Yap lives in the tray and the Yap bar — a mock of the
  // taskbar's corner (dark on purpose: it depicts Windows'), and what a
  // click and a right-click on the icon do.
  import yapIcon from '../../assets/yap-logo.svg';
  import StepHeader from './StepHeader.svelte';
</script>

<StepHeader title="Yap lives in your tray">
  Close the main window any time — Yap keeps running in the corner of your
  taskbar. The <strong>Yap bar</strong>, a small pill just above the taskbar,
  shows your words as you talk; hover it to dictate or take meeting notes.
</StepHeader>

<div class="tray-mock" aria-hidden="true">
  <div class="tray-arrow">⬇</div>
  <div class="tray-bar">
    <span class="tray-caret">^</span>
    <span class="tray-icon yap"><img class="tray-yap-img" src={yapIcon} alt="" /></span>
    <span class="tray-icon">☁</span>
    <span class="tray-lang">ENG</span>
    <span class="tray-icon">🔊</span>
    <span class="tray-clock">10:42</span>
  </div>
</div>
<ul class="tray-tips">
  <li><strong>Click</strong> the icon to open Yap.</li>
  <li><strong>Right-click</strong> it to paste your last dictation again, switch model or check for updates.</li>
  <li>The icon turns red while Yap is listening.</li>
</ul>

<style>
  .tray-mock {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    margin: 26px 0 18px;
  }
  .tray-arrow {
    color: var(--yap-primary);
    font-size: 20px;
    margin-bottom: 4px;
    margin-left: -132px;
  }
  /* The mock bar stays dark on purpose — it depicts the Windows taskbar. */
  .tray-bar {
    display: flex;
    align-items: center;
    gap: 14px;
    background: #1c2028;
    border: 1px solid #2a2f3a;
    border-radius: 10px;
    padding: 10px 18px;
    font-size: 13px;
    color: #9ca3af;
    box-shadow: var(--yap-shadow-menu);
  }
  .tray-caret {
    font-weight: 700;
  }
  .tray-icon.yap {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: 5px;
    background: #10131a;
    outline: 2px solid var(--yap-primary);
  }
  .tray-yap-img {
    width: 16px;
    height: 16px;
    border-radius: 4px;
    object-fit: contain;
  }
  .tray-clock {
    font-variant-numeric: tabular-nums;
  }
  .tray-tips {
    color: var(--yap-muted);
    font-size: 13px;
    line-height: 1.9;
    max-width: 400px;
    margin: 0 auto;
    padding-left: 18px;
  }
</style>
