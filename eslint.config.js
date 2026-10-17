import pluginVue from 'eslint-plugin-vue';
import { readFileSync } from 'node:fs';
import globals from 'globals';
import tseslint from 'typescript-eslint';

const legacyExemptions = JSON.parse(
    readFileSync(new URL('./eslint.legacy-exemptions.json', import.meta.url), 'utf8')
);

const legacyRuleOverrides = legacyExemptions.legacyFiles.map((file) => ({
    files: [file],
    rules: legacyExemptions.rules.reduce(
        (rules, rule) => ({ ...rules, [rule]: legacyExemptions.defaultSeverity }),
        {}
    )
}));

const formattingRules = {
    'vue/attribute-hyphenation': 'off',
    'vue/attributes-order': 'off',
    'vue/html-closing-bracket-newline': 'off',
    'vue/html-indent': 'off',
    'vue/html-self-closing': 'off',
    'vue/max-attributes-per-line': 'off',
    'vue/multiline-html-element-content-newline': 'off',
    'vue/mustache-interpolation-spacing': 'off',
    'vue/order-in-components': 'off',
    'vue/singleline-html-element-content-newline': 'off'
};

export default tseslint.config(
    {
        ignores: ['dist/**', 'node_modules/**', 'src-tauri/**']
    },
    ...tseslint.configs.recommended,
    {
        files: ['**/*.{js,mjs,cjs,ts,tsx}'],
        languageOptions: {
            globals: {
                ...globals.browser,
                ...globals.node
            },
            parserOptions: {
                sourceType: 'module'
            }
        },
        rules: {
            'no-control-regex': 'error',
            'no-empty': 'error'
        }
    },
    ...pluginVue.configs['flat/recommended-error'],
    {
        files: ['**/*.vue'],
        languageOptions: {
            globals: {
                ...globals.browser,
                ...globals.node
            },
            parserOptions: {
                extraFileExtensions: ['.vue'],
                parser: tseslint.parser,
                sourceType: 'module'
            }
        },
        rules: {
            ...formattingRules,
            'vue/multi-word-component-names': 'off',
            'vue/one-component-per-file': 'off',
            'vue/require-default-prop': 'off',
            'vue/no-unused-components': 'error',
            'vue/no-v-html': 'error',
            'vue/require-explicit-emits': 'error',
            'vue/require-prop-types': 'error'
        }
    },
    ...legacyRuleOverrides
);
