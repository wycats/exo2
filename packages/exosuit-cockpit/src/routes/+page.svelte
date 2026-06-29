<svelte:head>
  <title>Exo Cockpit</title>
  <meta
    name="description"
    content="Dormant Exo cockpit package awaiting the lane-centered workbench design"
  />
</svelte:head>

<main class="shell" aria-labelledby="cockpit-heading">
  <section class="panel">
    <p class="eyebrow">Exo Cockpit</p>
    <h1 id="cockpit-heading">Lane-centered workbench pending</h1>
    <p>
      This package remains as a buildable host shell while the next cockpit
      implementation follows the lane-centered workbench design package.
    </p>
  </section>
</main>

<style>
  :global(body) {
    margin: 0;
    background: #f7f4ed;
    color: #202426;
    font-family:
      Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI",
      sans-serif;
  }

  .shell {
    min-height: 100vh;
    display: grid;
    place-items: center;
    padding: 24px;
  }

  .panel {
    max-width: 560px;
    border: 1px solid #d6d0c4;
    border-radius: 8px;
    background: #fffdf8;
    padding: 28px;
  }

  .eyebrow {
    margin: 0 0 10px;
    color: #5d676b;
    font-size: 0.78rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  h1 {
    margin: 0;
    font-size: 2rem;
    line-height: 1.08;
    letter-spacing: 0;
  }

  p {
    margin: 14px 0 0;
    color: #41494d;
    line-height: 1.55;
  }
</style>
