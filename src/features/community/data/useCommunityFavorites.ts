import { favoriteBusy, favorites, loadFavorites, toggleProject } from './modFavorites';

export function useCommunityFavorites() {
    return {
        favorites,
        favoriteBusy,
        loadFavorites,
        toggleProject
    };
}
