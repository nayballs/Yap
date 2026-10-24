<script>
  // A setup step's header: Yap's mark on the first, the serif display
  // headline (as Settings' page titles) and the line under it (`children`).
  import yapIcon from '../../assets/yap-logo.svg';

  let { title, logo = false, children } = $props();
</script>

<header>
  {#if logo}
    <img class="logo" src={yapIcon} alt="" aria-hidden="true" />
  {/if}
  <h1>{title}</h1>
  <p class="sub">{@render children()}</p>
</header>

<style>
  header {
    text-align: center;
    margin-bottom: 18px;
  }
  .logo {
    width: 56px;
    height: 56px;
    margin: 0 auto 10px;
    border-radius: 14px;
    object-fit: contain;
    box-shadow: var(--yap-shadow-sm);
  }
  /* Serif display headline, matching the Settings page titles. */
  h1 {
    font-family: var(--yap-font-display);
    font-size: 26px;
    font-weight: 600;
    margin: 0 0 8px;
    letter-spacing: 0.01em;
  }
  .sub {
    color: var(--yap-muted);
    font-size: 13px;
    line-height: 1.6;
    max-width: 460px;
    margin: 0 auto;
  }
</style>
