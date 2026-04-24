import React from 'react';

export function AppIcon({ size = 24, className = '' }: { size?: number, className?: string }) {
  return (
    <svg 
      width={size} 
      height={size} 
      viewBox="0 0 24 24" 
      fill="none" 
      xmlns="http://www.w3.org/2000/svg"
      className={className}
    >
      {/* Downward arrow structure */}
      <path 
        d="M12 3V15M12 15L7.5 10.5M12 15L16.5 10.5" 
        stroke="currentColor" 
        strokeWidth="2.5" 
        strokeLinecap="round" 
        strokeLinejoin="round"
      />
      {/* Base tray / connection */}
      <path 
        d="M5 20C5 20 8 20 12 20C16 20 19 20 19 20" 
        stroke="currentColor" 
        strokeWidth="3" 
        strokeLinecap="round" 
        strokeLinejoin="round"
        className="text-primary opacity-80"
      />
    </svg>
  );
}
