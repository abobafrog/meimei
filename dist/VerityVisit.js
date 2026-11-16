/* Procedural WebGL Verity: a lit 3D sphere with a face, hat and bulbs. */
(() => {
  const section = document.querySelector('.best-snow');
  if (!section) return;

  const reduce = matchMedia('(prefers-reduced-motion: reduce)');
  let played = false;
  let visible = false;
  let run = 0;
  let frameId = 0;
  let animations = [];
  let savedScroll = null;

  function lockPageScroll() {
    if (savedScroll) return;
    const body = document.body;
    const root = document.documentElement;
    savedScroll = {
      x: window.scrollX,
      y: window.scrollY,
      rootOverflow: root.style.overflow,
      rootOverscroll: root.style.overscrollBehavior,
      body: {
        overflow: body.style.overflow,
        overscrollBehavior: body.style.overscrollBehavior,
      },
    };
    root.style.overflow = 'hidden';
    root.style.overscrollBehavior = 'none';
    body.style.overflow = 'hidden';
    body.style.overscrollBehavior = 'none';
    window.addEventListener('touchmove', preventPageScroll, { passive: false, capture: true });
    window.addEventListener('wheel', preventPageScroll, { passive: false, capture: true });
    window.addEventListener('keydown', preventScrollKeys, true);
  }

  function preventPageScroll(event) {
    if (event.cancelable) event.preventDefault();
  }

  function preventScrollKeys(event) {
    if (['ArrowUp', 'ArrowDown', 'PageUp', 'PageDown', 'Home', 'End', ' '].includes(event.key)) {
      event.preventDefault();
    }
  }

  function unlockPageScroll() {
    if (!savedScroll) return;
    const saved = savedScroll;
    const body = document.body;
    const root = document.documentElement;
    window.removeEventListener('touchmove', preventPageScroll, true);
    window.removeEventListener('wheel', preventPageScroll, true);
    window.removeEventListener('keydown', preventScrollKeys, true);
    root.style.overflow = saved.rootOverflow;
    root.style.overscrollBehavior = saved.rootOverscroll;
    for (const [property, value] of Object.entries(saved.body)) body.style[property] = value;
    savedScroll = null;
    if (Math.abs(window.scrollX - saved.x) > 2 || Math.abs(window.scrollY - saved.y) > 2) {
      window.scrollTo(saved.x, saved.y);
    }
  }

  const actor = document.createElement('div');
  actor.className = 'verity-visit';
  actor.hidden = true;
  actor.setAttribute('aria-hidden', 'true');
  const canvas = document.createElement('canvas');
  canvas.width = 320;
  canvas.height = 320;
  canvas.setAttribute('aria-hidden', 'true');
  actor.append(canvas);
  document.body.append(actor);

  const scene = createScene(canvas);
  let fallback = null;
  if (!scene) {
    canvas.hidden = true;
    fallback = createCssFallback();
    actor.append(fallback);
  }

  function stop() {
    run++;
    if (frameId) cancelAnimationFrame(frameId);
    frameId = 0;
    animations.forEach((animation) => animation.cancel());
    animations = [];
    actor.hidden = true;
    actor.className = 'verity-visit';
    unlockPageScroll();
  }

  async function animate(node, frames, options) {
    const animation = node.animate(frames, { fill: 'forwards', ...options });
    animations.push(animation);
    await animation.finished;
  }

  function drawWalk(token, startTime, duration) {
    const tick = (time) => {
      if (token !== run || actor.hidden) return;
      const progress = Math.min(1, (time - startTime) / duration);
      const turn = progress < 0.44
        ? 1.18 * (1 - progress / 0.44)
        : progress < 0.58
          ? 0
          : 1.18 * ((progress - 0.58) / 0.42);
      const step = Math.sin((time - startTime) * (Math.PI * 2 / 420));
      if (scene) drawScene(scene, turn, step * 0.035, step * 0.025);
      else fallback.style.transform = `translateY(${-Math.abs(step) * 4}px) rotateY(${turn}rad) rotateZ(${step * 2}deg)`;
      if (progress < 1) frameId = requestAnimationFrame(tick);
      else frameId = 0;
    };
    frameId = requestAnimationFrame(tick);
  }

  async function start() {
    if (played || !visible || document.hidden) return;
    played = true;
    const token = ++run;
    lockPageScroll();
    actor.hidden = false;

    try {
      if (reduce.matches) {
        drawScene(scene, 0, 0, 0);
        if (fallback) fallback.style.transform = 'rotateY(0deg)';
        actor.classList.add('verity-rest');
        await animate(actor, [{ opacity: 0 }, { opacity: 0.8 }, { opacity: 0 }], {
          duration: 2400,
        });
      } else {
        const duration = innerWidth < 600 ? 3300 : 3800;
        const movement = animate(
          actor,
          [
            { transform: 'translateX(-180px)', offset: 0 },
            { transform: 'translateX(calc(50vw - 50%))', offset: 0.44 },
            { transform: 'translateX(calc(50vw - 50%))', offset: 0.58 },
            { transform: 'translateX(calc(100vw + 24px))', offset: 1 },
          ],
          { duration, easing: 'linear' },
        );
        drawWalk(token, performance.now(), duration);
        await movement;
        if (token !== run) return;
        if (frameId) cancelAnimationFrame(frameId);
        frameId = 0;
        drawScene(scene, 0, 0, 0);
        if (fallback) fallback.style.transform = 'rotateY(0deg)';

        actor.hidden = true;
        await new Promise((resolve) => setTimeout(resolve, 220));
        if (token !== run) return;
        animations.forEach((animation) => animation.cancel());
        animations = [];
        actor.classList.add('verity-ghost');
        actor.hidden = false;
        drawScene(scene, 0, 0, 0);
        await animate(
          actor,
          [
            { opacity: 0, transform: 'scale(.98)' },
            { opacity: 0.94, transform: 'scale(1)', offset: 0.12 },
            { opacity: 0.88, transform: 'scale(1)', offset: 0.76 },
            { opacity: 0, transform: 'scale(1.02)' },
          ],
          { duration: 3200, easing: 'ease-out' },
        );
      }
    } catch {
      // Scrolling away or switching tabs cancels the current animation.
    } finally {
      if (token === run) stop();
    }
  }

  new IntersectionObserver(
    (entries) => {
      visible = entries[0].isIntersecting;
      if (visible) start();
      else if (!actor.hidden) stop();
    },
    { threshold: 0.18 },
  ).observe(section);

  document.addEventListener('visibilitychange', () => {
    if (document.hidden) stop();
    else start();
  });
  reduce.addEventListener('change', () => {
    if (reduce.matches) stop();
  });

  function createCssFallback() {
    const fallback = document.createElement('div');
    fallback.className = 'verity-fallback';
    fallback.innerHTML = '<span class="verity-fallback-hat"></span><span class="verity-fallback-brim"></span><span class="verity-fallback-pom"></span><span class="verity-fallback-eye eye-left"></span><span class="verity-fallback-eye eye-right"></span><span class="verity-fallback-smile"></span><span class="verity-fallback-bulb bulb-one"></span><span class="verity-fallback-bulb bulb-two"></span><span class="verity-fallback-bulb bulb-three"></span><span class="verity-fallback-bulb bulb-four"></span>';
    return fallback;
  }

  function createScene(target) {
    let gl;
    try {
      gl = target.getContext('webgl', { alpha: true, antialias: true, premultipliedAlpha: false });
      if (!gl) return null;

      const vertexSource = `
        attribute vec3 a_position;
        attribute vec3 a_normal;
        uniform mat4 u_mvp;
        uniform mat4 u_model;
        uniform mat3 u_normal;
        varying vec3 v_position;
        varying vec3 v_world;
        varying vec3 v_normal;
        void main() {
          v_position = a_position;
          vec4 world = u_model * vec4(a_position, 1.0);
          v_world = world.xyz;
          v_normal = normalize(u_normal * a_normal);
          gl_Position = u_mvp * vec4(a_position, 1.0);
        }
      `;
      const fragmentSource = `
        precision mediump float;
        uniform vec3 u_color;
        uniform float u_glow;
        uniform float u_face;
        varying vec3 v_position;
        varying vec3 v_world;
        varying vec3 v_normal;
        float eye(vec2 p, vec2 center) {
          vec2 q = (p - center) / vec2(0.065, 0.095);
          return 1.0 - smoothstep(0.82, 1.0, length(q));
        }
        void main() {
          vec3 n = normalize(v_normal);
          vec3 light = normalize(vec3(-0.55, 0.72, 1.15));
          vec3 view = normalize(vec3(0.0, 0.05, 3.6) - v_world);
          float diffuse = max(dot(n, light), 0.0);
          float specular = pow(max(dot(reflect(-light, n), view), 0.0), 38.0);
          float rim = pow(1.0 - max(dot(n, view), 0.0), 2.0);
          vec3 color = u_color * (0.42 + 0.68 * diffuse);
          color += vec3(1.0) * (0.42 * specular + 0.12 * rim);
          color += u_color * u_glow;

          if (u_face > 0.5 && v_position.z > 0.48) {
            float eyes = max(eye(v_position.xy, vec2(-0.23, 0.19)), eye(v_position.xy, vec2(0.23, 0.19)));
            float x = v_position.x;
            float curve = -0.30 + 0.23 * (x / 0.43) * (x / 0.43);
            float smile = (1.0 - smoothstep(0.018, 0.034, abs(v_position.y - curve))) * (1.0 - smoothstep(0.41, 0.44, abs(x)));
            color = mix(color, vec3(0.025, 0.035, 0.045), max(eyes, smile));
          }
          gl_FragColor = vec4(color, 1.0);
        }
      `;
      const program = makeProgram(gl, vertexSource, fragmentSource);
      if (!program) return null;
      gl.useProgram(program);
      gl.enable(gl.DEPTH_TEST);
      gl.enable(gl.BLEND);
      gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
      gl.clearColor(0, 0, 0, 0);

      const locations = {
        position: gl.getAttribLocation(program, 'a_position'),
        normal: gl.getAttribLocation(program, 'a_normal'),
        mvp: gl.getUniformLocation(program, 'u_mvp'),
        model: gl.getUniformLocation(program, 'u_model'),
        normalMatrix: gl.getUniformLocation(program, 'u_normal'),
        color: gl.getUniformLocation(program, 'u_color'),
        glow: gl.getUniformLocation(program, 'u_glow'),
        face: gl.getUniformLocation(program, 'u_face'),
      };

      const sphere = uploadMesh(gl, sphereGeometry(32, 48));
      const hat = uploadMesh(gl, coneGeometry(40, 0.56, 0.52, 0.31, 1.26));
      const wirePoints = [];
      for (let i = 0; i <= 100; i++) {
        const t = i / 100;
        const y = -0.18 - 0.43 * t;
        const radius = Math.sqrt(0.8 * 0.8 - y * y) + 0.02;
        const angle = -Math.PI * 0.55 + t * Math.PI * 3.7;
        wirePoints.push([radius * Math.sin(angle), y - 0.15, radius * Math.cos(angle)]);
      }
      const wire = uploadMesh(gl, tubeGeometry(wirePoints, 0.018, 6));
      const bulbs = [];
      const bulbColors = [
        [1.0, 0.12, 0.18], [1.0, 0.64, 0.05], [0.10, 0.62, 1.0],
        [0.22, 0.95, 0.44], [1.0, 0.18, 0.50], [1.0, 0.84, 0.12],
        [0.16, 0.72, 1.0], [0.48, 0.35, 1.0], [1.0, 0.25, 0.08],
        [0.15, 0.92, 0.55], [1.0, 0.58, 0.12], [0.34, 0.74, 1.0],
      ];
      for (let i = 0; i < bulbColors.length; i++) {
        const point = wirePoints[Math.round(i * (wirePoints.length - 1) / (bulbColors.length - 1))];
        bulbs.push({ point, color: bulbColors[i] });
      }
      return { gl, program, locations, sphere, hat, wire, bulbs, vp: null, canvas: target };
    } catch (error) {
      console.warn('Verity WebGL setup failed; using the CSS sphere fallback.', error);
      return null;
    }
  }

  function makeProgram(gl, vertexSource, fragmentSource) {
    const vertex = compileShader(gl, gl.VERTEX_SHADER, vertexSource);
    const fragment = compileShader(gl, gl.FRAGMENT_SHADER, fragmentSource);
    if (!vertex || !fragment) return null;
    const program = gl.createProgram();
    gl.attachShader(program, vertex);
    gl.attachShader(program, fragment);
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      console.warn('Verity shader link failed:', gl.getProgramInfoLog(program));
      gl.deleteProgram(program);
      return null;
    }
    return program;
  }

  function compileShader(gl, type, source) {
    const shader = gl.createShader(type);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      console.warn('Verity shader compile failed:', gl.getShaderInfoLog(shader));
      gl.deleteShader(shader);
      return null;
    }
    return shader;
  }

  function uploadMesh(gl, geometry) {
    const position = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, position);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(geometry.positions), gl.STATIC_DRAW);
    const normal = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, normal);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(geometry.normals), gl.STATIC_DRAW);
    const index = gl.createBuffer();
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, index);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, new Uint16Array(geometry.indices), gl.STATIC_DRAW);
    return { position, normal, index, count: geometry.indices.length };
  }

  function sphereGeometry(rows, columns) {
    const positions = [];
    const normals = [];
    const indices = [];
    for (let row = 0; row <= rows; row++) {
      const phi = row * Math.PI / rows;
      for (let column = 0; column <= columns; column++) {
        const theta = column * Math.PI * 2 / columns;
        const x = Math.sin(phi) * Math.cos(theta);
        const y = Math.cos(phi);
        const z = Math.sin(phi) * Math.sin(theta);
        positions.push(x, y, z);
        normals.push(x, y, z);
      }
    }
    for (let row = 0; row < rows; row++) {
      for (let column = 0; column < columns; column++) {
        const a = row * (columns + 1) + column;
        const b = a + columns + 1;
        indices.push(a, b, a + 1, b, b + 1, a + 1);
      }
    }
    return { positions, normals, indices };
  }

  function coneGeometry(segments, radius, baseY, tipX, tipY) {
    const positions = [];
    const normals = [];
    const indices = [];
    for (let i = 0; i < segments; i++) {
      const a = i * Math.PI * 2 / segments;
      const b = (i + 1) * Math.PI * 2 / segments;
      const p0 = [radius * Math.cos(a), baseY, radius * Math.sin(a)];
      const p1 = [radius * Math.cos(b), baseY, radius * Math.sin(b)];
      const tip = [tipX, tipY, 0];
      const normal = normalize(cross(subtract(tip, p0), subtract(p1, p0)));
      for (const point of [p0, p1, tip]) {
        positions.push(...point);
        normals.push(...normal);
      }
      const start = positions.length / 3 - 3;
      indices.push(start, start + 1, start + 2);
    }
    return { positions, normals, indices };
  }

  function tubeGeometry(points, radius, sides) {
    const positions = [];
    const normals = [];
    const indices = [];
    for (let i = 0; i < points.length; i++) {
      const tangent = normalize(subtract(points[Math.min(i + 1, points.length - 1)], points[Math.max(i - 1, 0)]));
      const reference = Math.abs(tangent[1]) > 0.92 ? [1, 0, 0] : [0, 1, 0];
      const axisA = normalize(cross(tangent, reference));
      const axisB = normalize(cross(tangent, axisA));
      for (let side = 0; side < sides; side++) {
        const angle = side * Math.PI * 2 / sides;
        const normal = [
          axisA[0] * Math.cos(angle) + axisB[0] * Math.sin(angle),
          axisA[1] * Math.cos(angle) + axisB[1] * Math.sin(angle),
          axisA[2] * Math.cos(angle) + axisB[2] * Math.sin(angle),
        ];
        positions.push(
          points[i][0] + normal[0] * radius,
          points[i][1] + normal[1] * radius,
          points[i][2] + normal[2] * radius,
        );
        normals.push(...normal);
      }
    }
    for (let i = 0; i < points.length - 1; i++) {
      for (let side = 0; side < sides; side++) {
        const next = (side + 1) % sides;
        const a = i * sides + side;
        const b = i * sides + next;
        const c = (i + 1) * sides + side;
        const d = (i + 1) * sides + next;
        indices.push(a, c, b, b, c, d);
      }
    }
    return { positions, normals, indices };
  }

  function drawScene(currentScene, yaw, bob, tilt) {
    if (!currentScene) return;
    const { gl, program, locations, canvas: target } = currentScene;
    const pixelRatio = Math.min(window.devicePixelRatio || 1, 2);
    const width = Math.round(target.clientWidth * pixelRatio) || 320;
    const height = Math.round(target.clientHeight * pixelRatio) || 320;
    if (target.width !== width || target.height !== height) {
      target.width = width;
      target.height = height;
    }
    gl.viewport(0, 0, width, height);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    gl.useProgram(program);

    const aspect = width / height;
    const projection = perspective(Math.PI / 4.0, aspect, 0.1, 20);
    const view = translation(0, 0, -3.65);
    const viewProjection = multiply(projection, view);
    const parentRotation = multiply(rotationY(yaw), rotationZ(tilt));
    const parent = multiply(translation(0, bob, 0), parentRotation);
    currentScene.vp = viewProjection;

    drawMesh(currentScene, currentScene.sphere, parent, [1.0, 0.68, 0.025], 0, 0.8, 0.8, 0.8, 1);
    drawMesh(currentScene, currentScene.wire, parent, [0.035, 0.19, 0.11], 0, 1, 1, 1);

    for (const bulb of currentScene.bulbs) {
      const model = multiply(parent, translation(...bulb.point));
      drawMesh(currentScene, currentScene.sphere, model, bulb.color, 0.65, 0.07, 0.085, 0.065);
    }

    drawMesh(currentScene, currentScene.sphere, multiply(parent, translation(0, 0.49, 0)), [0.88, 0.91, 0.9], 0.05, 0.62, 0.105, 0.59);
    drawMesh(currentScene, currentScene.hat, parent, [0.72, 0.035, 0.06], 0.02, 1, 1, 1);
    drawMesh(currentScene, currentScene.sphere, multiply(parent, translation(0.31, 1.26, 0)), [0.96, 0.97, 0.94], 0.1, 0.15, 0.16, 0.15);
  }

  function drawMesh(currentScene, mesh, objectTransform, color, glow, sx, sy, sz, face = 0) {
    const { gl, locations, vp } = currentScene;
    const scale = scaling(sx, sy, sz);
    const model = multiply(objectTransform, scale);
    const mvp = multiply(vp, model);
    const normal = normalMatrix(objectTransform, sx, sy, sz);

    gl.bindBuffer(gl.ARRAY_BUFFER, mesh.position);
    gl.enableVertexAttribArray(locations.position);
    gl.vertexAttribPointer(locations.position, 3, gl.FLOAT, false, 0, 0);
    gl.bindBuffer(gl.ARRAY_BUFFER, mesh.normal);
    gl.enableVertexAttribArray(locations.normal);
    gl.vertexAttribPointer(locations.normal, 3, gl.FLOAT, false, 0, 0);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, mesh.index);
    gl.uniformMatrix4fv(locations.mvp, false, mvp);
    gl.uniformMatrix4fv(locations.model, false, model);
    gl.uniformMatrix3fv(locations.normalMatrix, false, normal);
    gl.uniform3fv(locations.color, color);
    gl.uniform1f(locations.glow, glow);
    gl.uniform1f(locations.face, face);
    gl.drawElements(gl.TRIANGLES, mesh.count, gl.UNSIGNED_SHORT, 0);
  }

  function createCssFallback() {
    const fallback = document.createElement('div');
    fallback.className = 'verity-fallback';
    fallback.innerHTML = '<span class="verity-fallback-hat"></span><span class="verity-fallback-brim"></span><span class="verity-fallback-pom"></span><span class="verity-fallback-eye eye-left"></span><span class="verity-fallback-eye eye-right"></span><span class="verity-fallback-smile"></span><span class="verity-fallback-bulb bulb-one"></span><span class="verity-fallback-bulb bulb-two"></span><span class="verity-fallback-bulb bulb-three"></span><span class="verity-fallback-bulb bulb-four"></span>';
    return fallback;
  }

  function multiply(a, b) {
    const out = new Float32Array(16);
    for (let column = 0; column < 4; column++) {
      for (let row = 0; row < 4; row++) {
        let value = 0;
        for (let k = 0; k < 4; k++) value += a[k * 4 + row] * b[column * 4 + k];
        out[column * 4 + row] = value;
      }
    }
    return out;
  }

  function translation(x, y, z) {
    const out = identity();
    out[12] = x;
    out[13] = y;
    out[14] = z;
    return out;
  }

  function scaling(x, y, z) {
    const out = identity();
    out[0] = x;
    out[5] = y;
    out[10] = z;
    return out;
  }

  function rotationY(angle) {
    const out = identity();
    const c = Math.cos(angle);
    const s = Math.sin(angle);
    out[0] = c;
    out[2] = -s;
    out[8] = s;
    out[10] = c;
    return out;
  }

  function rotationZ(angle) {
    const out = identity();
    const c = Math.cos(angle);
    const s = Math.sin(angle);
    out[0] = c;
    out[1] = s;
    out[4] = -s;
    out[5] = c;
    return out;
  }

  function perspective(fov, aspect, near, far) {
    const f = 1 / Math.tan(fov / 2);
    const nf = 1 / (near - far);
    const out = new Float32Array(16);
    out[0] = f / aspect;
    out[5] = f;
    out[10] = (far + near) * nf;
    out[11] = -1;
    out[14] = 2 * far * near * nf;
    return out;
  }

  function normalMatrix(rotation, sx, sy, sz) {
    return new Float32Array([
      rotation[0] / sx, rotation[1] / sx, rotation[2] / sx,
      rotation[4] / sy, rotation[5] / sy, rotation[6] / sy,
      rotation[8] / sz, rotation[9] / sz, rotation[10] / sz,
    ]);
  }

  function identity() {
    return new Float32Array([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
  }

  function subtract(a, b) {
    return [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
  }

  function cross(a, b) {
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
  }

  function normalize(vector) {
    const length = Math.hypot(vector[0], vector[1], vector[2]) || 1;
    return [vector[0] / length, vector[1] / length, vector[2] / length];
  }
})();
