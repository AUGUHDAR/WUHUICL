/**
 * WUHUICL 本地图标：从 lucide（ISC License）SVG 提取的路径数据，包一层函数式 Vue 组件。
 * 对应 Axolotl 的 @modrinth/assets（同为 lucide 图标包），路径数据一字未改。
 */
import { defineComponent, h, type Component } from 'vue'

function lucideIcon(name: string, body: string): Component {
    return defineComponent({
        name,
        render() {
            return h(
                'svg',
                {
                    viewBox: '0 0 24 24',
                    width: '1em',
                    height: '1em',
                    fill: 'none',
                    stroke: 'currentColor',
                    'stroke-width': 2,
                    'stroke-linecap': 'round',
                    'stroke-linejoin': 'round',
                    innerHTML: body,
                },
            )
        },
    })
}

export const ArrowLeftRightIcon = lucideIcon('ArrowLeftRightIcon', '<path d="M8 3 4 7l4 4"/><path d="M4 7h16"/><path d="m16 21 4-4-4-4"/><path d="M20 17H4"/>')

export const ArrowUpDownIcon = lucideIcon('ArrowUpDownIcon', '<path d="m21 16-4 4-4-4"/><path d="M17 20V4"/><path d="m3 8 4-4 4 4"/><path d="M7 4v16"/>')

export const BoxIcon = lucideIcon('BoxIcon', '<path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"></path>\n  <polyline points="3.29 7 12 12 20.71 7"></polyline>\n  <line x1="12" y1="22" x2="12" y2="12"></line>')

export const BoxesIcon = lucideIcon('BoxesIcon', '<svg\n  class="lucide lucide-boxes"\n  xmlns="http://www.w3.org/2000/svg"\n  width="24"\n  height="24"\n  viewBox="0 0 24 24"\n  fill="none"\n  stroke="currentColor"\n  stroke-width="2"\n  stroke-linecap="round"\n  stroke-linejoin="round"\n>\n  <path d="M2.97 12.92A2 2 0 0 0 2 14.63v3.24a2 2 0 0 0 .97 1.71l3 1.8a2 2 0 0 0 2.06 0L12 19v-5.5l-5-3-4.03 2.42Z" />\n  <path d="m7 16.5-4.74-2.85" />\n  <path d="m7 16.5 5-3" />\n  <path d="M7 16.5v5.17" />\n  <path d="M12 13.5V19l3.97 2.38a2 2 0 0 0 2.06 0l3-1.8a2 2 0 0 0 .97-1.71v-3.24a2 2 0 0 0-.97-1.71L17 10.5l-5 3Z" />\n  <path d="m17 16.5-5-3" />\n  <path d="m17 16.5 4.74-2.85" />\n  <path d="M17 16.5v5.17" />\n  <path d="M7.97 4.42A2 2 0 0 0 7 6.13v4.37l5 3 5-3V6.13a2 2 0 0 0-.97-1.71l-3-1.8a2 2 0 0 0-2.06 0l-3 1.8Z" />\n  <path d="M12 8 7.26 5.15" />\n  <path d="m12 8 4.74-2.85" />\n  <path d="M12 13.5V8" />')

export const CheckIcon = lucideIcon('CheckIcon', '<path d="M20 6L9 17l-5-5" />')

export const ChevronDownIcon = lucideIcon('ChevronDownIcon', '<path d="m6 9 6 6 6-6" />')

export const ChevronLeftIcon = lucideIcon('ChevronLeftIcon', '<polyline points="15 18 9 12 15 6"></polyline>')

export const ChevronRightIcon = lucideIcon('ChevronRightIcon', '<polyline points="9 18 15 12 9 6"></polyline>')

export const CircleAlertIcon = lucideIcon('CircleAlertIcon', '<svg\n  class="lucide lucide-circle-alert"\n  xmlns="http://www.w3.org/2000/svg"\n  width="24"\n  height="24"\n  viewBox="0 0 24 24"\n  fill="none"\n  stroke="currentColor"\n  stroke-width="2"\n  stroke-linecap="round"\n  stroke-linejoin="round"\n>\n  <circle cx="12" cy="12" r="10" />\n  <line x1="12" x2="12" y1="8" y2="12" />\n  <line x1="12" x2="12.01" y1="16" y2="16" />')

export const ClipboardCopyIcon = lucideIcon('ClipboardCopyIcon', '<path stroke-linecap="round" stroke-linejoin="round" d="M8 5H6a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2v-1M8 5a2 2 0 002 2h2a2 2 0 002-2M8 5a2 2 0 012-2h2a2 2 0 012 2m0 0h2a2 2 0 012 2v3m2 4H10m0 0l3-3m-3 3l3 3" />')

export const CodeIcon = lucideIcon('CodeIcon', '<polyline points="16 18 22 12 16 6"></polyline><polyline points="8 6 2 12 8 18"></polyline>')

export const CompassIcon = lucideIcon('CompassIcon', '<path d="m16.24 7.76-1.804 5.411a2 2 0 0 1-1.265 1.265L7.76 16.24l1.804-5.411a2 2 0 0 1 1.265-1.265z"/><circle cx="12" cy="12" r="10"/>')

export const ContractIcon = lucideIcon('ContractIcon', '<path d="M9 5v4m0 0H5m4 0L4 4m11 1v4m0 0h4m-4 0 5-5M9 19v-4m0 0H5m4 0-5 5m11-5 5 5m-5-5v4m0-4h4" style="fill:none;stroke:currentColor;stroke-width:2;stroke-linecap:round;stroke-linejoin:round"/>')

export const CopyIcon = lucideIcon('CopyIcon', '<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>')

export const CubeIcon = lucideIcon('CubeIcon', '<path d="M10.362 1.093a.75.75 0 0 0-.724 0L2.523 5.018 10 9.143l7.477-4.125-7.115-3.925ZM18 6.443l-7.25 4v8.25l6.862-3.786A.75.75 0 0 0 18 14.25V6.443ZM9.25 18.693v-8.25l-7.25-4v7.807a.75.75 0 0 0 .388.657l6.862 3.786Z" />')

export const DatabaseIcon = lucideIcon('DatabaseIcon', '<ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M3 5V19A9 3 0 0 0 21 19V5"/><path d="M3 12A9 3 0 0 0 21 12"/>')

export const DownloadIcon = lucideIcon('DownloadIcon', '<path stroke-linecap="round" stroke-linejoin="round" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />')

export const EditIcon = lucideIcon('EditIcon', '<path\n\t\td="M11 5H6C4.89543 5 4 5.89543 4 7V18C4 19.1046 4.89543 20 6 20H17C18.1046 20 19 19.1046 19 18V13M17.5858 3.58579C18.3668 2.80474 19.6332 2.80474 20.4142 3.58579C21.1953 4.36683 21.1953 5.63316 20.4142 6.41421L11.8284 15H9L9 12.1716L17.5858 3.58579Z"\n\t\tstroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />')

export const ExpandIcon = lucideIcon('ExpandIcon', '<path stroke-linecap="round" stroke-linejoin="round" d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5l-5-5m5 5v-4m0 4h-4" />')

export const ExternalIcon = lucideIcon('ExternalIcon', '<path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"></path>\n    <polyline points="15 3 21 3 21 9"></polyline>\n    <line x1="10" y1="14" x2="21" y2="3"></line>')

export const EyeIcon = lucideIcon('EyeIcon', '<path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/>')

export const EyeOffIcon = lucideIcon('EyeOffIcon', '<path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/>')

export const FileArchiveIcon = lucideIcon('FileArchiveIcon', '<path d="M10 12v-1"/><path d="M10 18v-2"/><path d="M10 7V6"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="M15.5 22H18a2 2 0 0 0 2-2V7l-5-5H6a2 2 0 0 0-2 2v16a2 2 0 0 0 .274 1.01"/><circle cx="10" cy="20" r="2"/>')

export const FolderIcon = lucideIcon('FolderIcon', '<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>')

export const FolderSearchIcon = lucideIcon('FolderSearchIcon', '<path d="M11 20H4a2 2 0 0 1-2-2V5c0-1.1.9-2 2-2h3.93a2 2 0 0 1 1.66.9l.82 1.2a2 2 0 0 0 1.66.9H20a2 2 0 0 1 2 2v4"></path>\n  <circle cx="17" cy="17" r="3"></circle>\n  <path d="m21 21-1.5-1.5"></path>')

export const GridIcon = lucideIcon('GridIcon', '<rect x="3" y="3" width="7" height="7"></rect>\n  <rect x="14" y="3" width="7" height="7"></rect>\n  <rect x="14" y="14" width="7" height="7"></rect>\n  <rect x="3" y="14" width="7" height="7"></rect>')

export const HashIcon = lucideIcon('HashIcon', '<line x1="4" y1="9" x2="20" y2="9"></line><line x1="4" y1="15" x2="20" y2="15"></line><line x1="10" y1="3" x2="8" y2="21"></line><line x1="16" y1="3" x2="14" y2="21"></line>')

export const HistoryIcon = lucideIcon('HistoryIcon', '<path d="M3 3v5h5"></path>\n  <path d="M3.05 13A9 9 0 1 0 6 5.3L3 8"></path>\n  <path d="M12 7v5l4 2"></path>')

export const HomeIcon = lucideIcon('HomeIcon', '<path d="m3 9 9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"></path><polyline points="9 22 9 12 15 12 15 22"></polyline>')

export const ImageIcon = lucideIcon('ImageIcon', '<rect x="3" y="3" width="18" height="18" rx="2" ry="2"></rect>\n  <circle cx="9" cy="9" r="2"></circle>\n  <path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"></path>')

export const ImportIcon = lucideIcon('ImportIcon', '<path d="M12 3v12"/><path d="m8 11 4 4 4-4"/><path d="M8 5H4a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-4"/>')

export const InfoIcon = lucideIcon('InfoIcon', '<circle cx="12" cy="12" r="10"></circle><line x1="12" y1="16" x2="12" y2="12"></line><line x1="12" y1="8" x2="12.01" y2="8"></line>')

export const LandmarkIcon = lucideIcon('LandmarkIcon', '<path d="M10 18v-7" />\n\t<path d="M11.12 2.198a2 2 0 0 1 1.76.006l7.866 3.847c.476.233.31.949-.22.949H3.474c-.53 0-.695-.716-.22-.949z" />\n\t<path d="M14 18v-7" />\n\t<path d="M18 18v-7" />\n\t<path d="M3 22h18" />\n\t<path d="M6 18v-7" />')

export const LayersIcon = lucideIcon('LayersIcon', '<svg\n  class="lucide lucide-layers"\n  xmlns="http://www.w3.org/2000/svg"\n  width="24"\n  height="24"\n  viewBox="0 0 24 24"\n  fill="none"\n  stroke="currentColor"\n  stroke-width="2"\n  stroke-linecap="round"\n  stroke-linejoin="round"\n>\n  <path d="M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83z" />\n  <path d="M2 12a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 12" />\n  <path d="M2 17a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 17" />')

export const ListIcon = lucideIcon('ListIcon', '<rect x="3" y="14" width="7" height="7"></rect>\n  <rect x="3" y="3" width="7" height="7"></rect>\n  <line x1="14" y1="4" x2="21" y2="4"></line>\n  <line x1="14" y1="9" x2="21" y2="9"></line>\n  <line x1="14" y1="15" x2="21" y2="15"></line>\n  <line x1="14" y1="20" x2="21" y2="20"></line>')

export const MaximizeIcon = lucideIcon('MaximizeIcon', '<svg xmlns="http://www.w3.org/2000/svg" version="1.1" viewBox="0 0 24 24">\n  <!-- Generator: Adobe Illustrator 28.6.0, SVG Export Plug-In . SVG Version: 1.2.0 Build 709)  -->\n  <g>\n    <g id="Layer_1">\n      <g id="Layer_1-2" data-name="Layer_1">\n        <path d="M21,3H3v18h18V3Z" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="2"/>\n      </g>\n    </g>\n  </g>')

export const MinusIcon = lucideIcon('MinusIcon', '<path d="M5 12h14"/>')

export const MoreHorizontalIcon = lucideIcon('MoreHorizontalIcon', '<circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/><circle cx="5" cy="12" r="1"/>')

export const MoveIcon = lucideIcon('MoveIcon', '<svg\n  class="lucide lucide-move"\n  xmlns="http://www.w3.org/2000/svg"\n  width="24"\n  height="24"\n  viewBox="0 0 24 24"\n  fill="none"\n  stroke="currentColor"\n  stroke-width="2"\n  stroke-linecap="round"\n  stroke-linejoin="round"\n>\n  <path d="M12 2v20" />\n  <path d="m15 19-3 3-3-3" />\n  <path d="m19 9 3 3-3 3" />\n  <path d="M2 12h20" />\n  <path d="m5 9-3 3 3 3" />\n  <path d="m9 5 3-3 3 3" />')

export const PickaxeIcon = lucideIcon('PickaxeIcon', '<path d="M14.531 12.469 6.619 20.38a1 1 0 1 1-3-3l7.912-7.912"/><path d="M15.686 4.314A12.5 12.5 0 0 0 5.461 2.958 1 1 0 0 0 5.58 4.71a22 22 0 0 1 6.318 3.393"/><path d="M17.7 3.7a1 1 0 0 0-1.4 0l-4.6 4.6a1 1 0 0 0 0 1.4l2.6 2.6a1 1 0 0 0 1.4 0l4.6-4.6a1 1 0 0 0 0-1.4z"/><path d="M19.686 8.314a12.501 12.501 0 0 1 1.356 10.225 1 1 0 0 1-1.751-.119 22 22 0 0 0-3.393-6.319"/>')

export const PinIcon = lucideIcon('PinIcon', '<svg\n  class="lucide lucide-pin"\n  xmlns="http://www.w3.org/2000/svg"\n  width="24"\n  height="24"\n  viewBox="0 0 24 24"\n  fill="none"\n  stroke="currentColor"\n  stroke-width="2"\n  stroke-linecap="round"\n  stroke-linejoin="round"\n>\n  <path d="M12 17v5" />\n  <path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z" />')

export const PlusIcon = lucideIcon('PlusIcon', '<line x1="12" y1="5" x2="12" y2="19"></line>\n  <line x1="5" y1="12" x2="19" y2="12"></line>')

export const RedoIcon = lucideIcon('RedoIcon', '<path d="M21 7v6h-6"/><path d="M3 17a9 9 0 0 1 9-9 9 9 0 0 1 6 2.3l3 2.7"/>')

export const RefreshCwIcon = lucideIcon('RefreshCwIcon', '<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/>')

export const RotateClockwiseIcon = lucideIcon('RotateClockwiseIcon', '<path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/>')

export const RotateCounterClockwiseIcon = lucideIcon('RotateCounterClockwiseIcon', '<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>')

export const SaveIcon = lucideIcon('SaveIcon', '<path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"></path><polyline points="17 21 17 13 7 13 7 21"></polyline><polyline points="7 3 7 8 15 8"></polyline>')

export const ScaleIcon = lucideIcon('ScaleIcon', '<path stroke-linecap="round" stroke-linejoin="round" d="M3 6l3 1m0 0l-3 9a5.002 5.002 0 006.001 0M6 7l3 9M6 7l6-2m6 2l3-1m-3 1l-3 9a5.002 5.002 0 006.001 0M18 7l3 9m-3-9l-6-2m0-2v2m0 16V5m0 16H9m3 0h3" />')

export const ScanEyeIcon = lucideIcon('ScanEyeIcon', '<path d="M3 7V5a2 2 0 0 1 2-2h2"/><path d="M17 3h2a2 2 0 0 1 2 2v2"/><path d="M21 17v2a2 2 0 0 1-2 2h-2"/><path d="M7 21H5a2 2 0 0 1-2-2v-2"/><circle cx="12" cy="12" r="1"/><path d="M5 12s2.5-5 7-5 7 5 7 5-2.5 5-7 5-7-5-7-5"/>')

export const SearchIcon = lucideIcon('SearchIcon', '<path\n\t\td="M21 21L15 15M17 10C17 13.866 13.866 17 10 17C6.13401 17 3 13.866 3 10C3 6.13401 6.13401 3 10 3C13.866 3 17 6.13401 17 10Z"\n\t\tstroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />')

export const SettingsIcon = lucideIcon('SettingsIcon', '<circle cx="12" cy="12" r="3" />\n\t<path\n\t\td="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 010 2.83 2 2 0 01-2.83 0l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-2 2 2 2 0 01-2-2v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83 0 2 2 0 010-2.83l.06-.06a1.65 1.65 0 00.33-1.82 1.65 1.65 0 00-1.51-1H3a2 2 0 01-2-2 2 2 0 012-2h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 010-2.83 2 2 0 012.83 0l.06.06a1.65 1.65 0 001.82.33H9a1.65 1.65 0 001-1.51V3a2 2 0 012-2 2 2 0 012 2v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 0 2 2 0 010 2.83l-.06.06a1.65 1.65 0 00-.33 1.82V9a1.65 1.65 0 001.51 1H21a2 2 0 012 2 2 2 0 01-2 2h-.09a1.65 1.65 0 00-1.51 1z" />')

export const ShareIcon = lucideIcon('ShareIcon', '<circle cx="18" cy="5" r="3"/><circle cx="6" cy="12" r="3"/><circle cx="18" cy="19" r="3"/><line x1="8.59" x2="15.42" y1="13.51" y2="17.49"/><line x1="15.41" x2="8.59" y1="6.51" y2="10.49"/>')

export const SpinnerIcon = lucideIcon('SpinnerIcon', '<circle\n    opacity="0.25"\n    cx="12"\n    cy="12"\n    r="10"\n    stroke="currentColor"\n    stroke-width="4"\n  />\n  <path\n    opacity="0.75"\n    fill="currentColor"\n    d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"\n  />')

export const TrashIcon = lucideIcon('TrashIcon', '<polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path><line x1="10" y1="11" x2="10" y2="17"></line><line x1="14" y1="11" x2="14" y2="17"></line>')

export const TriangleAlertIcon = lucideIcon('TriangleAlertIcon', '<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3" />\n    <path d="M12 9v4" />\n    <path d="M12 17h.01" />')

export const UndoIcon = lucideIcon('UndoIcon', '<path d="M3 7v6h6"></path>\n  <path d="M21 17a9 9 0 0 0-9-9 9 9 0 0 0-6 2.3L3 13"></path>')

export const UnfoldVerticalIcon = lucideIcon('UnfoldVerticalIcon', '<svg\n  class="lucide lucide-unfold-vertical"\n  xmlns="http://www.w3.org/2000/svg"\n  width="24"\n  height="24"\n  viewBox="0 0 24 24"\n  fill="none"\n  stroke="currentColor"\n  stroke-width="2"\n  stroke-linecap="round"\n  stroke-linejoin="round"\n>\n  <path d="M12 22v-6" />\n  <path d="M12 8V2" />\n  <path d="M4 12H2" />\n  <path d="M10 12H8" />\n  <path d="M16 12h-2" />\n  <path d="M22 12h-2" />\n  <path d="m15 19-3 3-3-3" />\n  <path d="m15 5-3-3-3 3" />')

export const WorldIcon = lucideIcon('WorldIcon', '<path d="M21.54 15H17a2 2 0 0 0-2 2v4.54"/><path d="M7 3.34V5a3 3 0 0 0 3 3a2 2 0 0 1 2 2c0 1.1.9 2 2 2a2 2 0 0 0 2-2c0-1.1.9-2 2-2h3.17"/><path d="M11 21.95V18a2 2 0 0 0-2-2a2 2 0 0 1-2-2v-1a2 2 0 0 0-2-2H2.05"/><circle cx="12" cy="12" r="10"/>')

export const XIcon = lucideIcon('XIcon', '<path fill-rule="evenodd"\n    d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z"\n    clip-rule="evenodd" />')


export const TagCategoryFlagIcon = lucideIcon('TagCategoryFlagIcon', '<svg\n  class="lucide lucide-flag"\n  xmlns="http://www.w3.org/2000/svg"\n  width="24"\n  height="24"\n  viewBox="0 0 24 24"\n  fill="none"\n  stroke="currentColor"\n  stroke-width="2"\n  stroke-linecap="round"\n  stroke-linejoin="round"\n>\n  <path d="M4 22V4a1 1 0 0 1 .4-.8A6 6 0 0 1 8 2c3 0 5 2 7.333 2q2 0 3.067-.8A1 1 0 0 1 20 4v10a1 1 0 0 1-.4.8A6 6 0 0 1 16 16c-3 0-5-2-8-2a6 6 0 0 0-4 1.528" />')
