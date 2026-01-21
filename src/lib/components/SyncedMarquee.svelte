<script lang="ts" module>
  // Shared state for synced marquee groups
  // Using plain Map intentionally - this is a registry for animation coordination,
  // not reactive state. Updates are pushed via callbacks, not reactive reads.
  type MarqueeState = {
    offset: number;
    direction: "left" | "right";
    maxOffset: number;
  };

  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const groupStates = new Map<
    string,
    {
      state: MarqueeState;
      members: Set<{
        maxOffset: number;
        updateOffset: (offset: number) => void;
      }>;
      animationId: number | null;
      pauseTimeout: ReturnType<typeof setTimeout> | null;
    }
  >();

  const SCROLL_SPEED = 30; // pixels per second
  const PAUSE_DURATION = 2000; // ms to pause at each end

  function getGroup(groupId: string) {
    if (!groupStates.has(groupId)) {
      groupStates.set(groupId, {
        state: { offset: 0, direction: "left", maxOffset: 0 },
        members: new Set(),
        animationId: null,
        pauseTimeout: null,
      });
    }
    return groupStates.get(groupId)!;
  }

  function stopGroupAnimation(groupId: string) {
    const group = groupStates.get(groupId);
    if (!group) return;

    if (group.animationId !== null) {
      cancelAnimationFrame(group.animationId);
      group.animationId = null;
    }
    if (group.pauseTimeout !== null) {
      clearTimeout(group.pauseTimeout);
      group.pauseTimeout = null;
    }
  }

  function getGroupMaxOffset(group: { members: Set<{ maxOffset: number }> }) {
    let maxOffset = 0;
    for (const member of group.members) {
      if (member.maxOffset > maxOffset) {
        maxOffset = member.maxOffset;
      }
    }
    return maxOffset;
  }

  function startGroupAnimation(groupId: string) {
    const group = groupStates.get(groupId);
    if (!group || group.members.size === 0) return;

    const initialMaxOffset = getGroupMaxOffset(group);
    if (initialMaxOffset <= 0) return;

    group.state.maxOffset = initialMaxOffset;
    let lastTime: number | null = null;

    function animate(currentTime: number) {
      const group = groupStates.get(groupId);
      if (!group || group.members.size === 0) return;

      // Always get fresh max offset from members
      const groupMaxOffset = getGroupMaxOffset(group);
      if (groupMaxOffset <= 0) return;

      // Update state's maxOffset if it changed
      group.state.maxOffset = groupMaxOffset;

      if (lastTime === null) {
        lastTime = currentTime;
        group.animationId = requestAnimationFrame(animate);
        return;
      }

      const deltaTime = (currentTime - lastTime) / 1000;
      lastTime = currentTime;

      const movement = SCROLL_SPEED * deltaTime;
      const state = group.state;

      if (state.direction === "left") {
        state.offset += movement;
        if (state.offset >= state.maxOffset) {
          state.offset = state.maxOffset;
          state.direction = "right";
          // Pause at the end
          lastTime = null;

          // Update all members to their max position (100% progress)
          for (const member of group.members) {
            member.updateOffset(member.maxOffset);
          }

          group.pauseTimeout = setTimeout(() => {
            if (group.members.size > 0) {
              group.animationId = requestAnimationFrame(animate);
            }
          }, PAUSE_DURATION);
          return;
        }
      } else {
        state.offset -= movement;
        if (state.offset <= 0) {
          state.offset = 0;
          state.direction = "left";
          // Pause at the start
          lastTime = null;

          // Update all members with final position
          for (const member of group.members) {
            member.updateOffset(0);
          }

          group.pauseTimeout = setTimeout(() => {
            if (group.members.size > 0) {
              group.animationId = requestAnimationFrame(animate);
            }
          }, PAUSE_DURATION);
          return;
        }
      }

      // Update all members proportionally so they scroll together
      for (const member of group.members) {
        const progress =
          state.maxOffset > 0 ? state.offset / state.maxOffset : 0;
        const memberOffset = progress * member.maxOffset;
        member.updateOffset(Math.max(0, memberOffset));
      }

      group.animationId = requestAnimationFrame(animate);
    }

    // Reset state
    group.state.offset = 0;
    group.state.direction = "left";

    // Update all members to start position
    for (const member of group.members) {
      member.updateOffset(0);
    }

    // Initial pause before starting
    group.pauseTimeout = setTimeout(() => {
      group.animationId = requestAnimationFrame(animate);
    }, PAUSE_DURATION);
  }

  // Debounce restart to allow all members to register and calculate their offsets
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const restartTimeouts = new Map<string, ReturnType<typeof setTimeout>>();

  function restartGroupAnimation(groupId: string) {
    // Clear any pending restart
    const existingTimeout = restartTimeouts.get(groupId);
    if (existingTimeout) {
      clearTimeout(existingTimeout);
    }

    // Debounce: wait a frame for all members to calculate their offsets
    restartTimeouts.set(
      groupId,
      setTimeout(() => {
        restartTimeouts.delete(groupId);
        stopGroupAnimation(groupId);
        startGroupAnimation(groupId);
      }, 50)
    );
  }
</script>

<script lang="ts">
  interface Props {
    text: string;
    group: string;
    class?: string;
  }

  let { text, group: groupId, class: className = "" }: Props = $props();

  let containerRef: HTMLDivElement | null = $state(null);
  let textRef: HTMLSpanElement | null = $state(null);
  let offset = $state(0);
  let maxOffset = $state(0);
  let memberRef: {
    maxOffset: number;
    updateOffset: (offset: number) => void;
  } | null = null;

  function updateOffset(newOffset: number) {
    offset = newOffset;
  }

  function calculateMaxOffset() {
    if (containerRef && textRef) {
      const containerWidth = containerRef.clientWidth;
      const textWidth = textRef.scrollWidth;
      const newMaxOffset = Math.max(0, textWidth - containerWidth);

      if (newMaxOffset !== maxOffset) {
        maxOffset = newMaxOffset;
        if (memberRef) {
          memberRef.maxOffset = maxOffset;
        }
        // Restart animation when any member's size changes
        restartGroupAnimation(groupId);
      }
    }
  }

  // Register with group on mount
  $effect(() => {
    const group = getGroup(groupId);

    memberRef = {
      maxOffset,
      updateOffset,
    };

    group.members.add(memberRef);

    // Calculate initial offset after mount
    requestAnimationFrame(() => {
      calculateMaxOffset();
    });

    return () => {
      if (memberRef) {
        group.members.delete(memberRef);
      }

      // Clean up group if empty
      if (group.members.size === 0) {
        stopGroupAnimation(groupId);
        groupStates.delete(groupId);
      } else {
        // Restart animation for remaining members
        restartGroupAnimation(groupId);
      }
    };
  });

  // Recalculate on text change
  $effect(() => {
    void text;

    requestAnimationFrame(() => {
      calculateMaxOffset();
    });
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
