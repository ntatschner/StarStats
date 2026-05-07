/**
 * StarStats — quantum-jump background.
 * Side-on warp streaks: particles move along a directional vector with
 * parallax depth. The direction smoothly rotates whenever setWarpAngle()
 * is called (e.g. on screen change), so each page has its own flow.
 * Transparent canvas so the app UI sits naturally on top.
 */

(function () {
  const QuantumWarp = ({ angle = 180 }) => {
    const canvasRef = React.useRef(null);
    const stateRef = React.useRef({
      particles: [], w: 0, h: 0, accent: [232, 162, 60],
      angle: angle * Math.PI / 180,        // current
      targetAngle: angle * Math.PI / 180,  // tween toward this
    });

    // Update target angle whenever prop changes (smooth rotation)
    React.useEffect(() => {
      stateRef.current.targetAngle = angle * Math.PI / 180;
    }, [angle]);

    React.useEffect(() => {
      const canvas = canvasRef.current;
      if (!canvas) return;
      const ctx = canvas.getContext("2d", { alpha: true });
      const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

      let raf = 0;
      let running = true;

      const readAccent = () => {
        const root = document.getElementById("ss-root") || document.body;
        const v = getComputedStyle(root).getPropertyValue("--accent").trim() || "#E8A23C";
        if (v.startsWith("#")) {
          let h = v.slice(1);
          if (h.length === 3) h = h.split("").map((c) => c + c).join("");
          const n = parseInt(h, 16);
          return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
        }
        return [232, 162, 60];
      };

      const COUNT = 90;

      const spawnParticle = (p, fresh) => {
        const w = stateRef.current.w;
        const h = stateRef.current.h;
        p.depth = Math.random();
        // start position scattered if fresh, else just outside the entry edge
        // (entry edge is recomputed by the directional logic)
        p.x = fresh ? Math.random() * w : -100;
        p.y = fresh ? Math.random() * h : Math.random() * h;
        p.speed = 0.4 + p.depth * p.depth * 6.0;
        p.len = 18 + p.depth * 140;
        p.tint = Math.random() < 0.65 ? 1 : 0;
      };

      const init = () => {
        const dpr = Math.min(window.devicePixelRatio || 1, 2);
        const w = (stateRef.current.w = canvas.clientWidth);
        const h = (stateRef.current.h = canvas.clientHeight);
        canvas.width = Math.floor(w * dpr);
        canvas.height = Math.floor(h * dpr);
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
        if (stateRef.current.particles.length === 0) {
          stateRef.current.particles = Array.from({ length: COUNT }, () => {
            const p = {};
            spawnParticle(p, true);
            return p;
          });
        }
      };

      // Pick a fresh spawn position on the "incoming" edge of the viewport
      // given the current direction vector (dx, dy).
      const respawnEdge = (p, dx, dy, w, h) => {
        // pick an edge to spawn on biased by direction
        // we want the particle to enter from the opposite side of motion
        const margin = 40;
        // probability split: vertical edges weighted by |dx|, horizontal by |dy|
        const total = Math.abs(dx) + Math.abs(dy);
        if (Math.random() * total < Math.abs(dx)) {
          // enter on a vertical edge
          if (dx > 0) p.x = -margin - Math.random() * 60;
          else p.x = w + margin + Math.random() * 60;
          p.y = Math.random() * h;
        } else {
          if (dy > 0) p.y = -margin - Math.random() * 60;
          else p.y = h + margin + Math.random() * 60;
          p.x = Math.random() * w;
        }
      };

      const tick = () => {
        if (!running) return;
        const s = stateRef.current;
        const { particles, w, h, accent } = s;

        // Smoothly interpolate angle toward target (shortest path)
        let diff = s.targetAngle - s.angle;
        // wrap to [-PI, PI]
        while (diff > Math.PI) diff -= Math.PI * 2;
        while (diff < -Math.PI) diff += Math.PI * 2;
        s.angle += diff * 0.04; // 4% per frame -> ~25 frames to settle

        const dx = Math.cos(s.angle);
        const dy = Math.sin(s.angle);

        ctx.clearRect(0, 0, w, h);
        ctx.globalCompositeOperation = "lighter";
        ctx.lineCap = "round";

        const [ar, ag, ab] = accent;

        for (let i = 0; i < particles.length; i++) {
          const p = particles[i];
          p.x += dx * p.speed;
          p.y += dy * p.speed;

          // Recycle when off screen on the exit side
          if (p.x < -200 || p.x > w + 200 || p.y < -200 || p.y > h + 200) {
            spawnParticle(p, false);
            respawnEdge(p, dx, dy, w, h);
            continue;
          }

          const r = p.tint ? ar : 235;
          const g = p.tint ? ag : 235;
          const b = p.tint ? ab : 245;

          const baseAlpha = 0.05 + p.depth * 0.55;
          const width = 0.4 + p.depth * 1.6;

          // Tail trails behind in the opposite direction of motion
          const tailX = p.x - dx * p.len;
          const tailY = p.y - dy * p.len;

          const grad = ctx.createLinearGradient(p.x, p.y, tailX, tailY);
          grad.addColorStop(0, `rgba(${r},${g},${b},${baseAlpha})`);
          grad.addColorStop(0.6, `rgba(${r},${g},${b},${baseAlpha * 0.4})`);
          grad.addColorStop(1, `rgba(${r},${g},${b},0)`);

          ctx.strokeStyle = grad;
          ctx.lineWidth = width;
          ctx.beginPath();
          ctx.moveTo(p.x, p.y);
          ctx.lineTo(tailX, tailY);
          ctx.stroke();

          if (p.depth > 0.7) {
            const headA = (p.depth - 0.7) * 2.2;
            ctx.fillStyle = `rgba(${Math.min(255, r + 30)},${Math.min(255, g + 30)},${Math.min(255, b + 30)},${headA * baseAlpha * 1.4})`;
            ctx.beginPath();
            ctx.arc(p.x, p.y, width * 1.1, 0, Math.PI * 2);
            ctx.fill();
          }
        }

        if (!reduced) raf = requestAnimationFrame(tick);
      };

      stateRef.current.accent = readAccent();
      init();
      if (reduced) {
        for (let i = 0; i < 12; i++) tick();
      } else {
        raf = requestAnimationFrame(tick);
      }

      const onResize = () => init();
      window.addEventListener("resize", onResize);

      const accentInterval = setInterval(() => {
        stateRef.current.accent = readAccent();
      }, 600);

      return () => {
        running = false;
        cancelAnimationFrame(raf);
        window.removeEventListener("resize", onResize);
        clearInterval(accentInterval);
      };
    }, []);

    return (
      <canvas
        ref={canvasRef}
        className="ss-bg-warp-canvas"
        aria-hidden="true"
        style={{
          position: "fixed", inset: 0,
          width: "100%", height: "100%",
          pointerEvents: "none",
          zIndex: 0,
          opacity: 0.65,
        }}
      />
    );
  };

  window.QuantumWarp = QuantumWarp;
})();
