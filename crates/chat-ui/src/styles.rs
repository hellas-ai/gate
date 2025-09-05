//! Centralized style constants for consistent theming across the chat UI components

// Background colors with dark mode support
pub const CONTAINER_BG: &str = "bg-gray-50 dark:bg-gray-900";
pub const CARD_BG: &str = "bg-white dark:bg-gray-800";
pub const SECONDARY_BG: &str = "bg-gray-50 dark:bg-gray-700";
pub const TERTIARY_BG: &str = "bg-gray-100 dark:bg-gray-700";

// Text colors with dark mode support
pub const PRIMARY_TEXT: &str = "text-gray-900 dark:text-gray-100";
pub const SECONDARY_TEXT: &str = "text-gray-700 dark:text-gray-300";
pub const TERTIARY_TEXT: &str = "text-gray-600 dark:text-gray-400";
pub const MUTED_TEXT: &str = "text-gray-500 dark:text-gray-400";

// Border colors with dark mode support
pub const PRIMARY_BORDER: &str = "border-gray-200 dark:border-gray-700";
pub const SECONDARY_BORDER: &str = "border-gray-300 dark:border-gray-600";

// Status color pairs (background + text)
pub const ERROR_BG: &str = "bg-red-50 dark:bg-red-900";
pub const ERROR_TEXT: &str = "text-red-700 dark:text-red-300";
pub const ERROR_BORDER: &str = "border-red-200 dark:border-red-700";

pub const SUCCESS_BG: &str = "bg-green-50 dark:bg-green-900";
pub const SUCCESS_TEXT: &str = "text-green-800 dark:text-green-200";

pub const INFO_BG: &str = "bg-blue-50 dark:bg-blue-900";
pub const INFO_TEXT: &str = "text-blue-800 dark:text-blue-200";

pub const WARNING_BG: &str = "bg-orange-50 dark:bg-orange-900";
pub const WARNING_TEXT: &str = "text-orange-800 dark:text-orange-200";

// Message bubble styles (moved from message.rs)
pub const USER_BUBBLE_COLORS: &str =
    "bg-blue-100 dark:bg-blue-900 ml-10 md:ml-20 border border-blue-200 dark:border-blue-800";
pub const ASSISTANT_BUBBLE_COLORS: &str =
    "bg-white dark:bg-gray-700 mr-10 md:mr-20 border border-gray-200 dark:border-gray-600";
pub const SYSTEM_BUBBLE_COLORS: &str =
    "bg-orange-100 dark:bg-orange-900 italic border border-orange-200 dark:border-orange-800";
pub const TOOL_BUBBLE_COLORS: &str =
    "bg-blue-50 dark:bg-blue-900 font-mono text-sm border border-blue-200 dark:border-blue-800";
pub const DEFAULT_BUBBLE_COLORS: &str = "bg-white border border-gray-200 dark:border-gray-700";

// Button styles
pub const PRIMARY_BUTTON: &str = "px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white rounded-md transition-colors disabled:bg-gray-300 dark:disabled:bg-gray-600 disabled:cursor-not-allowed";
pub const SECONDARY_BUTTON: &str = "px-4 py-2 bg-gray-200 hover:bg-gray-300 dark:bg-gray-700 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-md transition-colors";
pub const DANGER_BUTTON: &str =
    "px-4 py-2 bg-red-500 hover:bg-red-600 text-white rounded-md transition-colors";

// Input styles
pub const INPUT_BASE: &str = "w-full px-3 py-2 border rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500";
pub const INPUT_COLORS: &str =
    "border-gray-300 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-200";
pub const TEXTAREA_BASE: &str = "w-full px-3 py-2 border rounded-md resize-none focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500";

// Code block styles
pub const CODE_BLOCK: &str = "bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded p-2 font-mono text-xs overflow-x-auto whitespace-pre-wrap";
pub const INLINE_CODE: &str = "bg-gray-100 dark:bg-gray-700 px-1 py-0.5 rounded text-sm font-mono";

// Common layout patterns
pub const FLEX_COL: &str = "flex flex-col";
pub const FLEX_COL_GAP_2: &str = "flex flex-col gap-2";
pub const FLEX_COL_GAP_4: &str = "flex flex-col gap-4";
pub const FLEX_CENTER: &str = "flex items-center";
pub const FLEX_CENTER_GAP_2: &str = "flex items-center gap-2";
pub const FLEX_BETWEEN: &str = "flex justify-between items-center";

// Common spacing
pub const CARD_PADDING: &str = "p-6";
pub const STANDARD_PADDING: &str = "p-4";
pub const HEADER_PADDING: &str = "px-4 py-3";
pub const BUTTON_PADDING: &str = "px-3 py-2";

// Shadows and rounded corners
pub const CARD_SHADOW: &str = "shadow-md";
pub const LIGHT_SHADOW: &str = "shadow-sm";
pub const ROUNDED_STANDARD: &str = "rounded-lg";
pub const ROUNDED_SMALL: &str = "rounded-md";

// Transitions
pub const TRANSITION_COLORS: &str = "transition-colors duration-200";
pub const TRANSITION_ALL: &str = "transition-all duration-200";

// Utility function to combine multiple style constants
pub fn combine_styles(styles: &[&str]) -> String {
    styles.join(" ")
}
