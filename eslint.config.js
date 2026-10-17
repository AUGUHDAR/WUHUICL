import pluginVue from 'eslint-plugin-vue';
import globals from 'globals';
import tseslint from 'typescript-eslint';

const vueRules = {
    'vue/attributes-order': 'off',
    'vue/attribute-hyphenation': 'off',
    'vue/block-order': 'off',
    'vue/html-closing-bracket-newline': 'off',
    'vue/html-indent': 'off',
    'vue/html-self-closing': 'off',
    'vue/max-attributes-per-line': 'off',
    'vue/multiline-html-element-content-newline': 'off',
    'vue/mustache-interpolation-spacing': 'off',
    'vue/multi-word-component-names': 'off',
    'vue/no-reserved-component-names': 'off',
    'vue/no-mutating-props': 'off',
    'vue/no-parsing-error': 'off',
    'vue/no-unused-components': 'off',
    'vue/no-v-html': 'off',
    'vue/one-component-per-file': 'off',
    'vue/order-in-components': 'off',
    'vue/require-default-prop': 'off',
    'vue/require-explicit-emits': 'off',
    'vue/require-prop-types': 'off',
    'vue/require-v-for-key': 'off',
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
            '@typescript-eslint/no-explicit-any': 'off',
            '@typescript-eslint/no-unused-vars': 'off',
            'no-control-regex': 'off',
            'no-empty': 'off',
            'no-undef': 'off',
            'no-unused-expressions': 'off',
            'no-unused-vars': 'off',
            'prefer-const': 'off'
        }
    },
    ...pluginVue.configs['flat/recommended'],
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
            ...vueRules,
            '@typescript-eslint/no-explicit-any': 'off',
            '@typescript-eslint/no-unused-vars': 'off',
            '@typescript-eslint/no-unused-expressions': 'off',
            'no-unused-expressions': 'off',
            'vue/no-dupe-keys': 'off'
        }
    }
);
