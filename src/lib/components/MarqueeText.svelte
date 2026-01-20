<script lang="ts">
  interface Props {
    text: string;
    class?: string;
  }

  let { text, class: className = "" }: Props = $props();

  let containerRef: HTMLDivElement | null = $state(null);
  let textRef: HTMLSpanElement | null = $state(null);
  let isOverflowing = $state(false);
  let offset = $state(0);
  let direction = $state<"left" | "right">("left");
  let animationId: number | null = null;
  let pauseTimeout: ReturnType<typeof setTimeout> | null = null;

  const SCROLL_SPEED = 30; // pixels per second
  const PAUSE_DURATION = 2000; // ms to pause at each end

  function checkOverflow() {
    if (containerRef && textRef) {
      const containerWidth = containerRef.clientWidth;
      const textWidth = textRef.scrollWidth;
      isOverflowing = textWidth > containerWidth;

      if (!isOverflowing) {
        offset = 0;
        direction = "left";
        stopAnimation();
      }
    }
  }

  function stopAnimation() {
    if (animationId !== null) {
      cancelAnimationFrame(animationId);
      animationId = null;
    }
    if (pauseTimeout !== null) {
      clearTimeout(pauseTimeout);
      pauseTimeout = null;
    }
  }

  function startAnimation() {
    if (!isOverflowing || !containerRef || !textRef) return;

    const containerWidth = containerRef.clientWidth;
    const textWidth = textRef.scrollWidth;
    const maxOffset = textWidth - containerWidth;

    let lastTime: number | null = null;

    function animate(currentTime: number) {
      if (!isOverflowing) return;

      if (lastTime === null) {
        lastTime = currentTime;
        animationId = requestAnimationFrame(animate);
        return;
      }

      const deltaTime = (currentTime - lastTime) / 1000; // seconds
      lastTime = currentTime;

      const movement = SCROLL_SPEED * deltaTime;

      if (direction === "left") {
        offset += movement;
        if (offset >= maxOffset) {
          offset = maxOffset;
          direction = "right";
          // Pause at the end
          lastTime = null;
          pauseTimeout = setTimeout(() => {
            if (isOverflowing) {
              animationId = requestAnimationFrame(animate);
            }
          }, PAUSE_DURATION);
          return;
        }
      } else {
        offset -= movement;
        if (offset <= 0) {
          offset = 0;
          direction = "left";
          // Pause at the start
          lastTime = null;
          pauseTimeout = setTimeout(() => {
            if (isOverflowing) {
              animationId = requestAnimationFrame(animate);
            }
          }, PAUSE_DURATION);
          return;
        }
      }

      animationId = requestAnimationFrame(animate);
    }

    // Initial pause before starting
    pauseTimeout = setTimeout(() => {
      animationId = requestAnimationFrame(animate);
    }, PAUSE_DURATION);
  }

  // Check overflow when text or container changes
  $effect(() => {
    // Track text changes
    void text;

    // Use requestAnimationFrame to ensure DOM is updated
    requestAnimationFrame(() => {
      checkOverflow();
    });
  });

  // Start/stop animation based on overflow state
  $effect(() => {
    if (isOverflowing) {
      offset = 0;
      direction = "left";
      startAnimation();
    } else {
      stopAnimation();
    }

    return () => {
      stopAnimation();
    };
  });

  // Cleanup on unmount
  $effect(() => {
    return () => {
      stopAnimation();
    };
  });
</script>

<div bind:this={containerRef} class="overflow-hidden {className}">
  <span
    bind:this={textRef}
    class="inline-block whitespace-nowrap"
    style="transform: translateX(-{offset}px)"
  >
    {text}
  </span>
</div>
