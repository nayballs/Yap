<script>
  // **A crew member** (Yap's own pixel characters: crew/sprites.js draws them,
  // crew/crew.css moves them, core/crew.js says who a model is): the Agent
  // panel's row avatar, the character holding its Mode's prop in a pose —
  // working, needs, done, stopped, failed, queued — with its badge. One
  // `role="img"` with its label ("Opus, working, Auto mode"); the markup is
  // the same string for the same props, so a row redrawn every second (its
  // clock) never restarts the walk. Its own chunk's (the panel's), never
  // the main window's.
  import './crew/crew.css';
  import { crewMarkup } from './crew/sprites.js';

  let { character = 'sonnet', prop = 'auto', pose = 'working', label = '', testid = '', size = 1 } = $props();

  const html = $derived(crewMarkup({ character, prop, pose, label, testid, size }));
</script>

{@html html}
