/** Hero artwork — lacquer mist: winged figure, coral-rose sun, waterfall.
 *  Shows Freya's image-making presence before a single generation is run. */
export default function HeroCanvas() {
  return (
    <svg
      className="hero-canvas-svg"
      viewBox="0 0 1600 900"
      preserveAspectRatio="xMidYMid slice"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
    >
      <defs>
        <linearGradient id="sky" x1="0" y1="0" x2="0.15" y2="1">
          <stop offset="0%" stopColor="#0e1418" />
          <stop offset="42%" stopColor="#162228" />
          <stop offset="74%" stopColor="#2c4540" />
          <stop offset="100%" stopColor="#3d2c2e" />
        </linearGradient>
        <linearGradient id="falls" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#e8e4dc" stopOpacity="0.95" />
          <stop offset="55%" stopColor="#7a9e95" stopOpacity="0.75" />
          <stop offset="100%" stopColor="#c5d5ce" stopOpacity="0.15" />
        </linearGradient>
        <linearGradient id="river" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#1c2b2e" />
          <stop offset="100%" stopColor="#0e1418" />
        </linearGradient>
        <filter id="soft" x="-20%" y="-20%" width="140%" height="140%">
          <feGaussianBlur stdDeviation="10" />
        </filter>
        <filter id="grain">
          <feTurbulence type="fractalNoise" baseFrequency="0.9" numOctaves="2" result="n" />
          <feColorMatrix in="n" type="matrix" values="0 0 0 0 1  0 0 0 0 0.95  0 0 0 0 0.85  0 0 0 0.06 0" />
          <feComposite operator="over" in2="SourceGraphic" />
        </filter>
      </defs>

      <rect width="1600" height="900" fill="url(#sky)" />

      {/* stars / print dust */}
      <g fill="#e8c4a8">
        <circle cx="140" cy="90" r="2.4" opacity="0.8" /><circle cx="330" cy="150" r="1.7" opacity="0.55" />
        <circle cx="520" cy="70" r="2" opacity="0.65" /><circle cx="760" cy="110" r="1.5" opacity="0.5" />
        <circle cx="1450" cy="80" r="2.2" opacity="0.7" /><circle cx="1330" cy="190" r="1.5" opacity="0.45" />
        <circle cx="90" cy="240" r="1.6" opacity="0.45" /><circle cx="640" cy="180" r="1.4" opacity="0.4" />
      </g>

      {/* coral-rose sun + pale-apricot rings */}
      <g className="hero-sun">
        <circle cx="1135" cy="235" r="218" fill="none" stroke="#e8c4a8" strokeWidth="2" opacity="0.5" />
        <circle cx="1135" cy="235" r="188" fill="none" stroke="#e8c4a8" strokeWidth="1.2" opacity="0.35" />
        <circle cx="1135" cy="235" r="158" fill="#d46a6a" />
        <circle cx="1135" cy="235" r="158" fill="#e8c4a8" opacity="0.12" />
      </g>

      {/* distant mesa */}
      <path d="M0 430 L260 385 L520 410 L820 360 L1120 395 L1600 350 L1600 560 L0 560 Z" fill="#18262c" opacity="0.9" />

      {/* canyon strata — left cliff */}
      <g>
        <path d="M0 300 L430 340 L470 900 L0 900 Z" fill="#2a2226" />
        <path d="M0 390 L445 425 L455 520 L0 480 Z" fill="#4f3538" />
        <path d="M0 520 L455 560 L462 650 L0 610 Z" fill="#7d5a52" opacity="0.85" />
        <path d="M0 660 L462 700 L470 900 L0 900 Z" fill="#141c22" />
        <path d="M0 300 L430 340 L438 385 L0 350 Z" fill="#e8c4a8" opacity="0.55" />
      </g>
      {/* canyon strata — right cliff */}
      <g>
        <path d="M1600 270 L1210 330 L1170 900 L1600 900 Z" fill="#241e22" />
        <path d="M1600 370 L1195 420 L1185 515 L1600 470 Z" fill="#4a3236" />
        <path d="M1600 545 L1182 585 L1176 680 L1600 635 Z" fill="#75504c" opacity="0.8" />
        <path d="M1600 690 L1174 720 L1170 900 L1600 900 Z" fill="#0e1418" />
        <path d="M1600 270 L1210 330 L1204 372 L1600 320 Z" fill="#d46a6a" opacity="0.6" />
      </g>

      {/* waterfall between the cliffs */}
      <g className="hero-falls">
        <path d="M742 405 C760 520 748 640 770 790 L852 790 C838 640 850 520 838 405 Z" fill="url(#falls)" />
        <path d="M782 410 C792 540 786 660 796 785" stroke="#ffffff" strokeWidth="7" fill="none" opacity="0.65" strokeLinecap="round" />
        <path d="M822 410 C830 530 826 650 834 780" stroke="#c5d5ce" strokeWidth="4" fill="none" opacity="0.5" strokeLinecap="round" />
        <ellipse cx="810" cy="800" rx="130" ry="26" fill="#c5d5ce" opacity="0.28" filter="url(#soft)" />
      </g>

      {/* river floor */}
      <rect y="790" width="1600" height="110" fill="url(#river)" />
      <ellipse cx="810" cy="822" rx="300" ry="18" fill="#7a9e95" opacity="0.16" />
      <ellipse cx="1135" cy="845" rx="220" ry="14" fill="#d46a6a" opacity="0.2" />

      {/* winged figure — ink silhouette with pale-apricot rim, standing centre-stage */}
      <g className="hero-figure">
        {/* far wing — scalloped feather trailing edge */}
        <path d="M1038 388 C930 268 790 178 585 148 C662 206 692 236 672 260 C742 268 792 296 778 324 C848 332 892 360 880 388 C948 396 1002 420 1038 438 Z" fill="#0b0f0d" />
        <path d="M1038 388 C930 268 790 178 585 148" fill="none" stroke="#e8c4a8" strokeWidth="3" opacity="0.7" strokeLinecap="round" />
        {/* near wing — scalloped feather trailing edge */}
        <path d="M1072 382 C1160 250 1306 158 1508 140 C1432 198 1402 230 1424 254 C1352 262 1302 290 1318 318 C1248 328 1204 356 1218 382 C1150 392 1104 414 1072 434 Z" fill="#111714" />
        <path d="M1072 382 C1160 250 1306 158 1508 140" fill="none" stroke="#e8c4a8" strokeWidth="3" opacity="0.85" strokeLinecap="round" />
        {/* feather cuts */}
        <g stroke="#e8e4dc" strokeWidth="2" opacity="0.16" strokeLinecap="round">
          <path d="M960 300 C900 250 840 218 770 198" /><path d="M980 348 C915 305 850 282 782 272" />
          <path d="M1150 292 C1210 242 1272 208 1344 190" /><path d="M1132 342 C1195 298 1262 272 1332 264" />
        </g>
        {/* staff */}
        <path d="M1108 402 L1172 618" stroke="#e8c4a8" strokeWidth="5" strokeLinecap="round" />
        <circle cx="1106" cy="394" r="9" fill="#e8c4a8" />
        {/* body / armour / skirt */}
        <path d="M1055 356 C1032 408 1024 470 1034 540 L1018 636 L1050 622 L1062 690 L1074 622 L1102 638 L1088 540 C1098 468 1088 406 1055 356 Z" fill="#0b0f0d" />
        <path d="M1038 452 L1084 452 L1080 478 L1042 478 Z" fill="#d46a6a" opacity="0.9" />
        <circle cx="1056" cy="330" r="27" fill="#0b0f0d" />
        <path d="M1030 322 C1042 306 1070 306 1082 322" fill="none" stroke="#e8c4a8" strokeWidth="4" strokeLinecap="round" />
        <path d="M1048 304 L1056 282 L1065 304" fill="none" stroke="#d46a6a" strokeWidth="4" strokeLinecap="round" strokeLinejoin="round" />
        {/* foreground rock she stands on */}
        <path d="M880 900 L950 682 L1150 668 L1245 900 Z" fill="#060c0a" />
      </g>

      {/* birds */}
      <g stroke="#0b0f0d" strokeWidth="4" fill="none" strokeLinecap="round" opacity="0.85">
        <path d="M250 190 q16 -14 32 0 q16 -14 32 0" />
        <path d="M360 240 q11 -10 22 0 q11 -10 22 0" />
      </g>

      <rect width="1600" height="900" fill="transparent" filter="url(#grain)" opacity="0.5" />
    </svg>
  );
}
